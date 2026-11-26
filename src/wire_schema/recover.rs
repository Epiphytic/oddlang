//! Recover a structural candidate from selected, source-pinned Serde declarations.
use super::{
    BINDING_FORMAT, BinaryList, Binding, Definition, FORMAT, Field, Payload, Result, Schema, Shape,
    Variant, digest, encode, io, iri, publish,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use syn::{Attribute, Expr, Fields, GenericArgument, Item, Lit, PathArguments, Token, Type};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    id: String,
    source_sha256: String,
    types: BTreeMap<String, String>,
    #[serde(default)]
    externals: BTreeMap<String, External>,
    #[serde(default)]
    adapters: BTreeMap<String, Adapter>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct External {
    id: String,
    rust_path: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Adapter {
    LowerHex { bytes: usize, diagnostic: String },
    BoundedBase64List { policy: BinaryList },
}

#[derive(Default)]
struct Serde {
    rename: Option<String>,
    rename_all: Option<String>,
    tag: Option<String>,
    content: Option<String>,
    adapter: Option<String>,
    deny_unknown: bool,
}

struct Observer<'a> {
    selection: &'a Selection,
    aliases: BTreeMap<String, &'a Type>,
    gaps: Vec<String>,
    binding: Binding,
}

impl Observer<'_> {
    fn serde(&mut self, at: &str, attrs: &[Attribute], allowed: &[&str]) -> Result<Serde> {
        let mut value = Serde::default();
        let mut seen = BTreeSet::new();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
            attr.parse_nested_meta(|entry| {
                let name = entry
                    .path
                    .get_ident()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                if !seen.insert(name.clone()) {
                    self.gaps
                        .push(format!("{at}: repeated Serde attribute {name}"));
                }
                if !allowed.contains(&name.as_str())
                    || !match name.as_str() {
                        "deny_unknown_fields" => {
                            entry.input.is_empty() || entry.input.peek(Token![,])
                        }
                        _ => entry.input.peek(Token![=]),
                    }
                {
                    self.gaps
                        .push(format!("{at}: unsupported Serde attribute {name}"));
                    if entry.input.peek(Token![=]) {
                        let _: Expr = entry.value()?.parse()?;
                    } else if entry.input.peek(syn::token::Paren) {
                        let content;
                        syn::parenthesized!(content in entry.input);
                        let _: proc_macro2::TokenStream = content.parse()?;
                    }
                    return Ok(());
                }
                if name == "deny_unknown_fields" {
                    value.deny_unknown = true;
                    return Ok(());
                }
                let text: syn::LitStr = entry.value()?.parse()?;
                match name.as_str() {
                    "rename" => value.rename = Some(text.value()),
                    "rename_all" => value.rename_all = Some(text.value()),
                    "tag" => value.tag = Some(text.value()),
                    "content" => value.content = Some(text.value()),
                    "with" => value.adapter = Some(text.value()),
                    _ => unreachable!("known string Serde attributes"),
                }
                Ok(())
            })
            .map_err(|error| format!("{at}: {error}"))?;
        }
        Ok(value)
    }

    fn conditional(&mut self, id: &str, attrs: &[Attribute]) {
        if attrs
            .iter()
            .any(|a| a.path().is_ident("cfg") || a.path().is_ident("cfg_attr"))
        {
            self.gaps.push(format!(
                "{id}: conditional declaration needs compiler evidence"
            ));
        }
    }

    fn derives(&mut self, id: &str, attrs: &[Attribute]) -> Result<()> {
        let mut host = BTreeSet::new();
        let mut serde = BTreeSet::new();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("derive")) {
            let paths = attr
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Path, Token![,]>::parse_terminated,
                )
                .map_err(|error| error.to_string())?;
            for path in paths {
                let name = path
                    .segments
                    .last()
                    .expect("nonempty path")
                    .ident
                    .to_string();
                match name.as_str() {
                    "Serialize" | "Deserialize" => {
                        serde.insert(name);
                    }
                    "Clone" | "Copy" | "Debug" | "Eq" | "PartialEq" => {
                        host.insert(name);
                    }
                    _ => self
                        .gaps
                        .push(format!("{id}: unsupported native derive {name}")),
                }
            }
        }
        if serde.len() != 2 {
            self.gaps.push(format!(
                "{id}: both Serde derives are required to recover the shape"
            ));
        }
        if !host.is_empty() {
            self.binding.derives.insert(id.to_owned(), host);
        }
        Ok(())
    }

    fn fields(&mut self, owner: &str, fields: &syn::FieldsNamed) -> Result<Vec<Field>> {
        fields
            .named
            .iter()
            .map(|field| {
                let rust = field.ident.as_ref().expect("named field").to_string();
                let rust = rust.strip_prefix("r#").unwrap_or(&rust).to_owned();
                let id = format!("{owner}:field:{rust}");
                self.conditional(&id, &field.attrs);
                let opts = self.serde(&id, &field.attrs, &["rename", "with"])?;
                let shape = if let Some(adapter) = opts.adapter {
                    match self.selection.adapters.get(&adapter) {
                        Some(Adapter::LowerHex { bytes, diagnostic })
                            if self.array_bytes(&field.ty, 0) == Some(*bytes) =>
                        {
                            Shape::LowerHex {
                                bytes: *bytes,
                                diagnostic: diagnostic.clone(),
                            }
                        }
                        Some(Adapter::BoundedBase64List { policy }) if vector_bytes(&field.ty) => {
                            Shape::BoundedBase64List {
                                policy: policy.clone(),
                            }
                        }
                        _ => self.unresolved(&id, format!("unverified Serde adapter {adapter}")),
                    }
                } else {
                    self.shape(&id, &field.ty)
                };
                self.binding.names.insert(id.clone(), rust.clone());
                Ok(Field {
                    id,
                    name: opts.rename.unwrap_or(rust),
                    shape,
                })
            })
            .collect()
    }

    fn unresolved(&mut self, at: &str, reason: String) -> Shape {
        self.gaps.push(format!("{at}: {reason}"));
        Shape::Unresolved { reason }
    }

    fn array_bytes(&self, ty: &Type, depth: usize) -> Option<usize> {
        if depth > self.aliases.len() {
            return None;
        }
        match ty {
            Type::Array(array) => {
                let Type::Path(element) = array.elem.as_ref() else {
                    return None;
                };
                if !element.path.is_ident("u8") {
                    return None;
                }
                let Expr::Lit(length) = &array.len else {
                    return None;
                };
                let Lit::Int(length) = &length.lit else {
                    return None;
                };
                length.base10_parse().ok()
            }
            Type::Path(path) => {
                let alias = path.path.get_ident()?.to_string();
                self.array_bytes(self.aliases.get(&alias)?, depth + 1)
            }
            _ => None,
        }
    }

    fn shape(&mut self, at: &str, ty: &Type) -> Shape {
        let Type::Path(path) = ty else {
            return self.unresolved(at, "unsupported non-path field type".into());
        };
        if path.qself.is_some() {
            return self.unresolved(at, "unsupported associated field type".into());
        }
        let name = path
            .path
            .segments
            .iter()
            .map(|part| part.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        let leaf = path.path.segments.last().expect("nonempty path");
        if let PathArguments::AngleBracketed(args) = &leaf.arguments {
            let params: Vec<_> = args.args.iter().collect();
            if let [GenericArgument::Type(item)] = params.as_slice() {
                if matches!(name.as_str(), "Vec" | "std::vec::Vec" | "alloc::vec::Vec") {
                    return Shape::List {
                        item: Box::new(self.shape(at, item)),
                    };
                }
                if matches!(
                    name.as_str(),
                    "Option" | "std::option::Option" | "core::option::Option"
                ) {
                    return Shape::Optional {
                        item: Box::new(self.shape(at, item)),
                    };
                }
            }
            return self.unresolved(at, format!("unsupported generic field type {name}"));
        }
        match name.as_str() {
            "u8" => Shape::Unsigned { bits: 8 },
            "u16" => Shape::Unsigned { bits: 16 },
            "u32" => Shape::Unsigned { bits: 32 },
            "u64" => Shape::Unsigned { bits: 64 },
            "bool" => Shape::Bool {},
            "String" | "std::string::String" => Shape::Text {},
            _ => {
                if let Some(id) = self.selection.types.get(&name) {
                    return Shape::Ref { id: id.clone() };
                }
                if let Some(external) = self.selection.externals.get(&name) {
                    return Shape::Ref {
                        id: external.id.clone(),
                    };
                }
                self.unresolved(at, format!("unresolved native reference {name}"))
            }
        }
    }

    fn variant(&mut self, owner: &str, value: &syn::Variant, snake_case: bool) -> Result<Variant> {
        let rust = value.ident.to_string();
        let rust = rust.strip_prefix("r#").unwrap_or(&rust).to_owned();
        let id = format!("{owner}:variant:{rust}");
        self.conditional(&id, &value.attrs);
        let opts = self.serde(&id, &value.attrs, &["rename"])?;
        let wire = opts.rename.unwrap_or_else(|| {
            if snake_case {
                let mut wire = String::new();
                for (index, ch) in rust.chars().enumerate() {
                    if index != 0 && ch.is_uppercase() {
                        wire.push('_');
                    }
                    wire.push(ch.to_ascii_lowercase());
                }
                wire
            } else {
                rust.clone()
            }
        });
        let payload = match &value.fields {
            Fields::Unit => Payload::Unit {},
            Fields::Named(fields) => Payload::Record {
                fields: self.fields(&id, fields)?,
            },
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let field = &fields.unnamed[0];
                self.conditional(&id, &field.attrs);
                if field.attrs.iter().any(|attr| attr.path().is_ident("serde")) {
                    self.gaps.push(format!(
                        "{id}: unsupported Serde attributes on newtype payload"
                    ));
                }
                Payload::Newtype {
                    shape: self.shape(&id, &field.ty),
                }
            }
            Fields::Unnamed(_) => {
                self.gaps.push(format!("{id}: unsupported tuple variant"));
                Payload::Unit {}
            }
        };
        self.binding.names.insert(id.clone(), rust);
        Ok(Variant {
            id,
            name: wire,
            payload,
        })
    }
}

fn vector_bytes(ty: &Type) -> bool {
    fn vector_item(ty: &Type) -> Option<&Type> {
        let Type::Path(path) = ty else { return None };
        let leaf = path.path.segments.last()?;
        let names = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        if path.qself.is_some()
            || !matches!(names.as_str(), "Vec" | "std::vec::Vec" | "alloc::vec::Vec")
        {
            return None;
        }
        let PathArguments::AngleBracketed(args) = &leaf.arguments else {
            return None;
        };
        let arguments: Vec<_> = args.args.iter().collect();
        let [GenericArgument::Type(inner)] = arguments.as_slice() else {
            return None;
        };
        Some(inner)
    }
    matches!(vector_item(ty).and_then(vector_item), Some(Type::Path(leaf)) if leaf.path.is_ident("u8"))
}

pub fn from_source(source: &Path, selection: &Path, output: &Path) -> Result<()> {
    let selection_bytes = io(fs::read(selection))?;
    let config: Selection =
        serde_json::from_slice(&selection_bytes).map_err(|error| error.to_string())?;
    let source = io(fs::read(source))?;
    let source_sha256 = digest(&source);
    if source_sha256 != config.source_sha256 {
        return Err("wire recovery source digest mismatch".into());
    }
    iri(&config.id)?;
    let mut selected_ids = BTreeSet::new();
    for id in config
        .types
        .values()
        .chain(config.externals.values().map(|external| &external.id))
    {
        iri(id)?;
        if !selected_ids.insert(id) {
            return Err("recovery selection needs distinct type identities".into());
        }
    }
    if config.types.is_empty() {
        return Err("select at least one Rust declaration".into());
    }
    let parsed = syn::parse_file(std::str::from_utf8(&source).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let mut observer = Observer {
        selection: &config,
        aliases: parsed
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Type(alias) if alias.generics.params.is_empty() => {
                    Some((alias.ident.to_string(), alias.ty.as_ref()))
                }
                _ => None,
            })
            .collect(),
        gaps: Vec::new(),
        binding: Binding {
            format: BINDING_FORMAT.into(),
            names: BTreeMap::new(),
            derives: BTreeMap::new(),
            externals: config
                .externals
                .values()
                .map(|external| (external.id.clone(), external.rust_path.clone()))
                .collect(),
        },
    };
    let mut types = Vec::new();
    for item in &parsed.items {
        let (name, attrs, generics) = match item {
            Item::Struct(item) => (item.ident.to_string(), &item.attrs, &item.generics),
            Item::Enum(item) => (item.ident.to_string(), &item.attrs, &item.generics),
            _ => continue,
        };
        let Some(id) = config.types.get(&name) else {
            continue;
        };
        observer.derives(id, attrs)?;
        observer.conditional(id, attrs);
        if !generics.params.is_empty() {
            observer.gaps.push(format!(
                "{id}: parameterized declaration needs compiler evidence"
            ));
        }
        observer.binding.names.insert(id.clone(), name);
        let shape = match item {
            Item::Struct(item) => {
                let state = observer.serde(id, attrs, &["deny_unknown_fields"])?;
                if !state.deny_unknown {
                    observer.gaps.push(format!("{id}: record is not closed"));
                }
                let Fields::Named(fields) = &item.fields else {
                    observer
                        .gaps
                        .push(format!("{id}: only named record fields are supported"));
                    continue;
                };
                Definition::Record {
                    id: id.clone(),
                    fields: observer.fields(id, fields)?,
                }
            }
            Item::Enum(item) => {
                let state = observer.serde(
                    id,
                    attrs,
                    &["tag", "content", "rename_all", "deny_unknown_fields"],
                )?;
                let (Some(tag), Some(content)) = (state.tag, state.content) else {
                    observer
                        .gaps
                        .push(format!("{id}: union is not explicitly adjacent-tagged"));
                    continue;
                };
                if !state.deny_unknown
                    || state
                        .rename_all
                        .as_deref()
                        .is_some_and(|rule| rule != "snake_case")
                {
                    observer.gaps.push(format!(
                        "{id}: union closure or rename rule is outside the profile"
                    ));
                }
                let snake = state.rename_all.as_deref() == Some("snake_case");
                let variants = item
                    .variants
                    .iter()
                    .map(|variant| observer.variant(id, variant, snake))
                    .collect::<Result<Vec<_>>>()?;
                Definition::Adjacent {
                    id: id.clone(),
                    tag,
                    content,
                    variants,
                }
            }
            _ => unreachable!("selected struct or enum"),
        };
        types.push(shape);
    }
    if config.types.len()
        != observer
            .binding
            .names
            .keys()
            .filter(|id| config.types.values().any(|v| v == *id))
            .count()
    {
        return Err("selected Rust declaration is absent from the pinned source".into());
    }
    types.extend(
        config
            .externals
            .values()
            .map(|external| Definition::External {
                id: external.id.clone(),
            }),
    );
    types.sort_by(|left, right| left.id().cmp(right.id()));
    observer.gaps.sort();
    observer.gaps.dedup();
    let mut gaps: Vec<Value> = observer
        .gaps
        .iter()
        .map(|reason| json!({"kind":"structure","reason":reason}))
        .collect();
    gaps.push(
        json!({"kind":"behavior","reason":"conditional guards require independent evidence"}),
    );
    let schema = Schema {
        format: FORMAT.into(),
        id: config.id.clone(),
        types,
        guards: Vec::new(),
        unresolved: observer.gaps,
    };
    let graph = encode(&schema)?;
    let binding = encode(&observer.binding)?;
    let report = encode(&json!({
        "qualification":"structural-candidate",
        "sourceSha256":source_sha256,
        "selectionSha256":digest(&selection_bytes),
        "graphSha256":digest(&graph),
        "rustBindingSha256":digest(&binding),
        "gaps":gaps
    }))?;
    publish(
        output,
        &[
            ("graph.json", &graph),
            ("rust.json", &binding),
            ("observation.json", &report),
        ],
    )
}
