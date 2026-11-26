//! Projection and recovery of the synchronous value-only WIT profile.
use super::model::{Map, Node, Res, World};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use wit_parser::{FunctionKind, Resolve, Type, TypeDefKind, TypeOwner, WorldItem, WorldKey};

fn ident(n: &str) -> String {
    match n {
        "package" | "world" | "interface" | "import" | "export" | "include" | "use" | "with"
        | "as" | "type" | "record" | "enum" | "variant" | "flags" | "resource" | "own"
        | "borrow" | "func" | "async" | "future" | "stream" | "result" | "option" | "list"
        | "tuple" | "map" | "bool" | "string" | "char" | "s8" | "s16" | "s32" | "s64" | "u8"
        | "u16" | "u32" | "u64" | "f32" | "f64" | "static" | "constructor" | "from"
        | "error-context" => format!("%{n}"),
        _ => n.into(),
    }
}
pub(crate) fn external(s: &str) -> Res<()> {
    let (path, v) = s.split_once('@').ok_or("unpinned foreign WIT package")?;
    let (pkg, iface) = path.split_once('/').ok_or("invalid foreign WIT path")?;
    let (ns, name) = pkg.split_once(':').ok_or("invalid foreign WIT package")?;
    for n in [ns, name, iface] {
        wit_parser::validate_id(n).map_err(|_| "invalid foreign WIT identifier")?
    }
    if v.is_empty()
        || !v
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".+-".contains(&c))
    {
        return Err("invalid foreign WIT version".into());
    }
    Ok(())
}
/// Parse a type expression independently of its enclosing WIT declaration.
pub(crate) fn type_expr(s: &str) -> Res<String> {
    fn parse(s: &mut &str, depth: usize) -> Res<String> {
        if depth > 128 {
            return Err("WIT type nesting limit".into());
        }
        *s = s.trim_start();
        let n = s
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '%' | '_')))
            .unwrap_or(s.len());
        let (word, rest) = s.split_at(n);
        *s = rest.trim_start();
        if word.is_empty() {
            return Err("missing WIT type".into());
        }
        if matches!(word, "option" | "list" | "result") {
            if !s.starts_with('<') {
                return if word == "result" {
                    Ok("result".into())
                } else {
                    Err(format!("invalid {word} type"))
                };
            }
            *s = &s[1..];
            let first = parse(s, depth + 1)?;
            let mut args = first.clone();
            if word == "result" && s.starts_with(',') {
                *s = &s[1..];
                let second = parse(s, depth + 1)?;
                if second == "_" {
                    return Err("invalid result error type".into());
                }
                args += &format!(", {second}");
            } else if first == "_" {
                return Err("invalid WIT type placeholder".into());
            }
            *s = s
                .strip_prefix('>')
                .ok_or("expected closing WIT type bracket")?
                .trim_start();
            return Ok(format!("{word}<{args}>"));
        }
        match word {
            "bool" | "string" | "s8" | "s16" | "s32" | "s64" | "_" => Ok(word.into()),
            "resource" | "own" | "borrow" | "async" | "future" | "stream" | "tuple" | "flags"
            | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" | "char" | "map" | "error-context" => {
                Err(format!("unsupported Wasm construct {word}"))
            }
            _ => {
                let name = word.strip_prefix('%').unwrap_or(word);
                wit_parser::validate_id(name).map_err(|_| "invalid WIT type identifier")?;
                Ok(ident(name))
            }
        }
    }
    let mut rest = s;
    let ty = parse(&mut rest, 0)?;
    if !rest.is_empty() || ty == "_" {
        return Err(format!("invalid WIT type {s}"));
    }
    Ok(ty)
}

impl World {
    fn named(&self, id: &str) -> String {
        ident(&self.nodes[id].name)
    }
    fn signature(&self, n: &Node) -> Res<String> {
        let params = self
            .ordered(n, "param")?
            .into_iter()
            .map(|id| {
                Ok(format!(
                    "{}: {}",
                    self.named(id),
                    type_expr(&self.nodes[id].props["valueType"])?
                ))
            })
            .collect::<Res<Vec<_>>>()?
            .join(", ");
        let ret = n
            .props
            .get("result")
            .map(|s| type_expr(s))
            .transpose()?
            .map(|s| format!(" -> {s}"))
            .unwrap_or_default();
        Ok(format!("{}: func({params}){ret};", ident(&n.name)))
    }
    fn uses(&self, n: &Node) -> Res<String> {
        let mut out = String::new();
        for id in n.targets("use") {
            let t = &self.nodes[id];
            let owner = if t.kind == "ForeignType" {
                t.props["from"].clone()
            } else {
                let mut owners = self
                    .nodes
                    .values()
                    .filter(|i| i.kind == "Interface" && i.targets("type").any(|x| x == id));
                let first = owners.next().ok_or("type use has no declaring interface")?;
                if owners.next().is_some() {
                    return Err("ambiguous type owner".into());
                }
                first
                    .props
                    .get("from")
                    .cloned()
                    .unwrap_or_else(|| ident(&first.name))
            };
            out += &format!("  use {owner}.{{{}}};\n", ident(&t.name));
        }
        Ok(out)
    }
    pub fn render(&self) -> Res<String> {
        self.identities()?;
        let package = self.package_name();
        let mut out = format!("package {package};\n");
        for n in self
            .nodes
            .values()
            .filter(|n| n.kind == "Interface" && !n.props.contains_key("from"))
        {
            out += &format!("\ninterface {} {{\n", ident(&n.name));
            out += &self.uses(n)?;
            for id in n.targets("type") {
                let t = &self.nodes[id];
                let keyword = t.kind.to_ascii_lowercase();
                out += &format!("  {keyword} {} {{\n", ident(&t.name));
                let record = t.kind == "Record";
                for member in self.ordered(t, if record { "field" } else { "case" })? {
                    let m = &self.nodes[member];
                    let name = ident(&m.name);
                    let field = match m.props.get("valueType") {
                        Some(v) if record => format!("{name}: {}", type_expr(v)?),
                        Some(v) => format!("{name}({})", type_expr(v)?),
                        None => name,
                    };
                    out += &format!("    {field},\n");
                }
                out += "  }\n";
            }
            for id in n.targets("function") {
                out += &format!("  {}\n", self.signature(&self.nodes[id])?)
            }
            out += "}\n";
        }
        for (id, source) in self.nodes.iter().filter(|(_, n)| n.kind == "World") {
            let n = Node {
                links: self.effective_world(id)?,
                ..source.clone()
            };
            out += &format!("\nworld {} {{\n", ident(&n.name));
            out += &self.uses(&n)?;
            for k in ["import", "export"] {
                for id in n.targets(k) {
                    let item = &self.nodes[id];
                    let sig = if item.kind == "Function" {
                        self.signature(item)?
                    } else {
                        format!(
                            "{};",
                            item.props
                                .get("from")
                                .cloned()
                                .unwrap_or_else(|| self.named(id))
                        )
                    };
                    out += &format!("  {k} {sig}\n");
                }
            }
            out += "}\n";
        }
        Ok(out)
    }
}

fn unsupported(k: &str) -> String {
    format!("unsupported Wasm construct {k}")
}
fn ty(r: &Resolve, t: Type) -> Res<String> {
    Ok(match t {
        Type::Bool => "bool".into(),
        Type::S8 => "s8".into(),
        Type::S16 => "s16".into(),
        Type::S32 => "s32".into(),
        Type::S64 => "s64".into(),
        Type::String => "string".into(),
        Type::Id(id) => {
            let d = &r.types[id];
            if let Some(n) = &d.name {
                return Ok(ident(n));
            }
            match &d.kind {
                TypeDefKind::List(t) => format!("list<{}>", ty(r, *t)?),
                TypeDefKind::Option(t) => format!("option<{}>", ty(r, *t)?),
                TypeDefKind::Result(v) => match (v.ok, v.err) {
                    (None, None) => "result".into(),
                    (Some(t), None) => format!("result<{}>", ty(r, t)?),
                    (ok, Some(e)) => format!(
                        "result<{}, {}>",
                        ok.map(|t| ty(r, t))
                            .transpose()?
                            .unwrap_or_else(|| "_".into()),
                        ty(r, e)?
                    ),
                },
                TypeDefKind::Type(t) => ty(r, *t)?,
                k => return Err(unsupported(k.as_str())),
            }
        }
        _ => return Err(unsupported(&format!("{t:?}").to_lowercase())),
    })
}
struct Recover<'a> {
    resolve: &'a Resolve,
    ids: &'a Map,
    namespace: Option<&'a str>,
    nodes: BTreeMap<String, Node>,
    seen: BTreeSet<String>,
    definitions: Map,
}
impl Recover<'_> {
    fn declare(&mut self, path: &str, kind: &str, name: &str) -> Res<String> {
        self.seen.insert(path.into());
        let id = match self.ids.get(path) {
            Some(id) => id.clone(),
            None => format!(
                "{}{path}",
                self.namespace
                    .ok_or_else(|| format!("missing stable IRI mapping {path}"))?
            ),
        };
        if let Some(previous) = self.definitions.insert(id.clone(), path.into())
            && previous != path
        {
            return Err(format!(
                "distinct declarations share stable IRI {id}: {previous} and {path}"
            ));
        }
        oxigraph::model::NamedNode::new(&id)
            .map_err(|_| format!("invalid declaration IRI {id}"))?;
        let node = self.nodes.entry(id.clone()).or_insert_with(|| Node {
            kind: kind.into(),
            name: name.into(),
            ..Default::default()
        });
        if node.kind != kind || node.name != name {
            return Err(format!("conflicting stable IRI {id}"));
        }
        Ok(id)
    }
    fn edge(&mut self, parent: &str, k: &str, child: &str) {
        self.nodes.get_mut(parent).unwrap().link(k, child)
    }
    fn prop(&mut self, id: &str, k: &str, v: String) {
        self.nodes.get_mut(id).unwrap().props.insert(k.into(), v);
    }
    fn func(&mut self, path: &str, f: &wit_parser::Function) -> Res<String> {
        if f.kind != FunctionKind::Freestanding {
            return Err(unsupported("function kind"));
        }
        let id = self.declare(path, "Function", &f.name)?;
        for (i, p) in f.params.iter().enumerate() {
            let child = self.declare(&format!("{path}/param/{}", p.name), "Param", &p.name)?;
            self.prop(&child, "ordinal", i.to_string());
            self.prop(&child, "valueType", ty(self.resolve, p.ty)?);
            self.edge(&id, "param", &child);
        }
        if let Some(t) = f.result {
            self.prop(&id, "result", ty(self.resolve, t)?);
        }
        Ok(id)
    }
    fn datatype(&mut self, path: &str, name: &str, id: wit_parser::TypeId) -> Res<String> {
        let d = &self.resolve.types[id];
        let (kind, members) = match &d.kind {
            TypeDefKind::Record(v) => (
                "Record",
                v.fields
                    .iter()
                    .map(|m| (&m.name, Some(m.ty)))
                    .collect::<Vec<_>>(),
            ),
            TypeDefKind::Variant(v) => {
                ("Variant", v.cases.iter().map(|m| (&m.name, m.ty)).collect())
            }
            TypeDefKind::Enum(v) => ("Enum", v.cases.iter().map(|m| (&m.name, None)).collect()),
            k => return Err(unsupported(k.as_str())),
        };
        let parent = self.declare(path, kind, name)?;
        let (key, member_kind) = if kind == "Record" {
            ("field", "Field")
        } else {
            ("case", "Case")
        };
        for (i, (n, t)) in members.into_iter().enumerate() {
            let child = self.declare(&format!("{path}/{key}/{n}"), member_kind, n)?;
            self.prop(&child, "ordinal", i.to_string());
            if let Some(t) = t {
                self.prop(&child, "valueType", ty(self.resolve, t)?);
            }
            self.edge(&parent, key, &child);
        }
        Ok(parent)
    }
    fn use_type(
        &mut self,
        path: &str,
        name: &str,
        alias: wit_parser::TypeId,
        pkg: wit_parser::PackageId,
    ) -> Res<String> {
        let TypeDefKind::Type(Type::Id(target)) = self.resolve.types[alias].kind else {
            return Err(unsupported("world-local type definition"));
        };
        let d = &self.resolve.types[target];
        if d.name.as_deref() != Some(name) {
            return Err(unsupported("renamed type use"));
        }
        let TypeOwner::Interface(owner) = d.owner else {
            return Err(unsupported("type owner"));
        };
        let i = &self.resolve.interfaces[owner];
        let local = i.package == Some(pkg);
        let owner_name = if local {
            i.name
                .clone()
                .ok_or_else(|| unsupported("inline interface"))?
        } else {
            self.resolve
                .id_of(owner)
                .ok_or("missing foreign package name")?
        };
        let owner_path = format!("interface/{owner_name}");
        if local || self.ids.contains_key(&owner_path) {
            let resolved = self.datatype(&format!("{owner_path}/type/{name}"), name, target)?;
            // Alias paths name the same semantic declaration, including its members.
            self.alias_tree(path, &resolved)?;
            Ok(resolved)
        } else {
            let id = self.declare(path, "ForeignType", name)?;
            let from = self
                .resolve
                .id_of(owner)
                .ok_or("missing foreign package name")?;
            external(&from)?;
            self.prop(&id, "from", from);
            Ok(id)
        }
    }
    fn alias_tree(&mut self, path: &str, id: &str) -> Res<()> {
        self.seen.insert(path.into());
        if let Some(s) = self.ids.get(path)
            && s != id
        {
            return Err(format!("conflicting stable IRI mapping {path}"));
        }
        if self.namespace.is_none() && !self.ids.contains_key(path) {
            return Err(format!("missing stable IRI mapping {path}"));
        }
        let n = self.nodes[id].clone();
        for (k, targets) in n.links {
            for child in targets {
                self.alias_tree(&format!("{path}/{k}/{}", self.nodes[&child].name), &child)?
            }
        }
        Ok(())
    }
}

pub fn import_dir(dir: &Path, world: &str, ids: &Map, namespace: Option<&str>) -> Res<World> {
    if !dir.is_dir() {
        return Err("unsupported Wasm input: expected a WIT directory".into());
    }
    let mut r = Resolve::default();
    let (pkg, _) = r.push_dir(dir).map_err(|e| format!("invalid WIT: {e:#}"))?;
    let w_id = r
        .select_world(&[pkg], Some(world))
        .map_err(|e| format!("invalid WIT world: {e:#}"))?;
    let w = &r.worlds[w_id];
    if w.package != Some(pkg) {
        return Err("selected world belongs to a dependency".into());
    }
    let mut c = Recover {
        resolve: &r,
        ids,
        namespace,
        nodes: BTreeMap::new(),
        seen: BTreeSet::new(),
        definitions: Map::new(),
    };
    let package = c.declare("package", "Package", &r.packages[pkg].name.to_string())?;
    let selected = c.declare("world", "World", &w.name)?;
    let mut interfaces = BTreeMap::new();
    for (i_id, iface) in r.interfaces.iter() {
        let Some(name) = iface.name.as_ref() else {
            continue;
        };
        let local = iface.package == Some(pkg);
        let owner = if local {
            name.clone()
        } else {
            r.id_of(i_id).ok_or("missing foreign WIT interface")?
        };
        let path = format!("interface/{owner}");
        if !local && !ids.contains_key(&path) {
            continue;
        }
        let parent = c.declare(&path, "Interface", name)?;
        if !local {
            external(&owner)?;
            c.prop(&parent, "from", owner);
        }
        interfaces.insert(i_id, parent.clone());
        for (n, t) in &iface.types {
            let (key, id) = if matches!(r.types[*t].kind, TypeDefKind::Type(Type::Id(_))) {
                ("use", c.use_type(&format!("{path}/use/{n}"), n, *t, pkg)?)
            } else {
                ("type", c.datatype(&format!("{path}/type/{n}"), n, *t)?)
            };
            c.edge(&parent, key, &id)
        }
        for (n, f) in &iface.functions {
            let id = c.func(&format!("{path}/function/{n}"), f)?;
            c.edge(&parent, "function", &id)
        }
    }
    let mut implicit: BTreeSet<_> = w
        .imports
        .values()
        .filter_map(|v| {
            let WorldItem::Type { id, .. } = v else {
                return None;
            };
            let TypeDefKind::Type(Type::Id(t)) = r.types[*id].kind else {
                return None;
            };
            if let TypeOwner::Interface(i) = r.types[t].owner {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    for item in w.imports.values().chain(w.exports.values()) {
        if let WorldItem::Interface { id, .. } = item {
            implicit.extend(r.interface_direct_deps(*id));
        }
    }
    for (direction, items) in [("import", &w.imports), ("export", &w.exports)] {
        for (key, item) in items {
            let (k, id) = match item {
                WorldItem::Function(f) => (
                    direction,
                    c.func(&format!("world/{direction}/{}", f.name), f)?,
                ),
                WorldItem::Type { id, .. } => {
                    let n = r.types[*id].name.as_deref().ok_or("unnamed WIT type")?;
                    ("use", c.use_type(&format!("world/use/{n}"), n, *id, pkg)?)
                }
                WorldItem::Interface { id, .. } => {
                    let i = &r.interfaces[*id];
                    let name = i
                        .name
                        .as_deref()
                        .ok_or_else(|| unsupported("inline interface"))?;
                    let path = format!("world/{direction}/{name}");
                    if direction == "import"
                        && implicit.contains(id)
                        && i.functions.is_empty()
                        && !ids.contains_key(&path)
                    {
                        continue;
                    }
                    if !matches!(key, WorldKey::Interface(_)) {
                        return Err(unsupported("named interface alias"));
                    }
                    let node = interfaces
                        .get(id)
                        .ok_or_else(|| unsupported("foreign interface function binding"))?;
                    c.alias_tree(&path, node)?;
                    (direction, node.clone())
                }
            };
            c.edge(&selected, k, &id);
        }
    }
    if let Some(path) = ids.keys().find(|p| !c.seen.contains(*p)) {
        return Err(format!("unused stable IRI mapping {path}"));
    }
    let result = World {
        nodes: c.nodes,
        package,
        selected,
    };
    World::from_turtle(result.to_turtle()?.as_bytes())
}

pub fn recover(dir: &Path, world: &str, identity_source: &str, out: &Path) -> Res<()> {
    recover_locked(dir, world, identity_source, out, None)
}

pub(super) fn recover_locked(
    dir: &Path,
    world: &str,
    identity_source: &str,
    out: &Path,
    lock: Option<&Path>,
) -> Res<()> {
    let (ids, namespace) =
        if identity_source.starts_with("urn:") || identity_source.starts_with("https://") {
            (Map::new(), Some(identity_source))
        } else {
            let bytes = fs::read(identity_source).map_err(|e| e.to_string())?;
            (
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
                None,
            )
        };
    let w = if let Some(lock) = lock {
        let wit_dir = fs::canonicalize(dir).map_err(|e| e.to_string())?;
        if wit_dir.file_name() != Some("wit".as_ref()) {
            return Err("expected published release WIT directory".into());
        }
        let root = wit_dir.parent().ok_or("missing release directory")?;
        let external = fs::canonicalize(lock).map_err(|e| e.to_string())?;
        if external.starts_with(root) {
            return Err("supply an independently pinned workspace lock outside the release".into());
        }
        let (published, files) = super::bundle::verified_wit(root, &external)?;
        if published.name() != world {
            return Err("selected world disagrees with release pin".into());
        }
        if namespace.is_none() && ids != published.identities()? {
            return Err("identity map disagrees with pinned release".into());
        }
        let stage = tempfile::tempdir().map_err(|e| e.to_string())?;
        for (name, bytes) in files {
            let path = stage.path().join(name);
            fs::create_dir_all(path.parent().unwrap())
                .and_then(|_| fs::write(path, bytes))
                .map_err(|e| e.to_string())?;
        }
        import_dir(stage.path(), world, &ids, namespace)?
    } else {
        let w = import_dir(dir, world, &ids, namespace)?;
        if w.nodes.values().any(|n| n.props.contains_key("from")) {
            return Err("foreign WIT recovery requires an external workspace --lock".into());
        }
        w
    };
    if out.exists()
        && fs::read_dir(out)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err("recovery needs an empty output directory".into());
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    let dest = out;
    let out = stage.path();
    fs::write(out.join("model.ttl"), w.to_turtle()?).map_err(|e| e.to_string())?;
    fs::write(
        out.join("identities.json"),
        serde_json::to_vec_pretty(&w.identities()?).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        out.join("missing-evidence.json"),
        b"{\"missing\":[\"behavior\",\"constraints\"]}\n",
    )
    .map_err(|e| e.to_string())?;
    fs::rename(out, dest).map_err(|e| e.to_string())
}
