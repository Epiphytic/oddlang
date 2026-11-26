use super::*;
use syn::{
    Attribute, Expr, Fields, GenericArgument, Item, Lit, Meta, PathArguments, Type, Variant,
};

pub(super) fn extract(source: &str, selection: Selection) -> Result<Graph, String> {
    let file = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut definitions = BTreeMap::new();
    for item in &file.items {
        let name = match item {
            Item::Struct(item) => &item.ident,
            Item::Enum(item) => &item.ident,
            _ => continue,
        };
        if definitions.insert(name.to_string(), item).is_some() {
            return Err(format!("ambiguous declaration {name}"));
        }
    }
    let Some(Item::Struct(root)) = definitions.get(&selection.root) else {
        return Err(format!("unknown parser {}", selection.root));
    };
    require_derive(&root.attrs, "Parser")?;
    reject_conditional(&root.attrs)?;
    if !attrs(&root.attrs, "group")?.is_empty() || !matches!(root.fields, Fields::Named(_)) {
        return Err("unsupported parser structure or implicit root group override".into());
    }
    let mut graph = Graph {
        profile: "command-surface-v1".into(),
        package_name: selection.package_name,
        version: selection.version,
        root: Command::default(),
        external: false,
        terminal_subparsers: BTreeMap::new(),
        gaps: selection.gaps,
    };
    graph.root.name = graph.package_name.clone();
    graph.root.help = help(&root.attrs)?;
    let mut version = false;
    for meta in attrs(&root.attrs, "command")? {
        match key(&meta)?.as_str() {
            "name" => graph.root.name = string(value(&meta)?)?,
            "about" => graph.root.help = Some(string(value(&meta)?)?),
            "version" if matches!(meta, Meta::Path(_)) => version = true,
            name => return Err(format!("unsupported parser setting {name}")),
        }
    }
    if version == graph.version.is_empty() {
        return Err("parser version declaration differs from the recovery selection".into());
    }
    let mut subcommand_type = None;
    for field in &root.fields {
        reject_conditional(&field.attrs)?;
        let command = attrs(&field.attrs, "command")?;
        if command.is_empty() {
            if let Some(argument) = argument(field, &definitions)? {
                graph.root.arguments.push(argument);
            }
            continue;
        }
        if command.len() != 1
            || !command[0].path().is_ident("subcommand")
            || subcommand_type.is_some()
        {
            return Err("expected one declared subcommand field".into());
        }
        let (outer, inner) = type_name(&field.ty)?;
        graph.root.subcommand_required = outer != "Option";
        subcommand_type = Some(inner.unwrap_or(outer));
    }
    if !graph.root.arguments.is_empty() {
        graph.root.groups.push(Group {
            id: selection.root,
            arguments: graph
                .root
                .arguments
                .iter()
                .map(|arg| arg.id.clone())
                .collect(),
            multiple: true,
            ..Group::default()
        });
    }
    if let Some(name) = subcommand_type {
        let Some(Item::Enum(commands)) = definitions.get(&name) else {
            return Err(format!("unresolved subcommand enum {name}"));
        };
        require_derive(&commands.attrs, "Subcommand")?;
        reject_conditional(&commands.attrs)?;
        if !attrs(&commands.attrs, "command")?.is_empty() {
            return Err("unsupported subcommand enum settings".into());
        }
        for variant in &commands.variants {
            if let Some(command) = command(variant, &definitions)? {
                graph.root.subcommands.push(command);
            } else {
                if graph.external {
                    return Err("multiple external subcommands".into());
                }
                graph.external = true;
            }
        }
    }
    Ok(graph)
}

type Definitions<'a> = BTreeMap<String, &'a Item>;

fn command(variant: &Variant, definitions: &Definitions<'_>) -> Result<Option<Command>, String> {
    reject_conditional(&variant.attrs)?;
    let mut command = Command {
        name: kebab(&variant.ident.to_string()),
        help: help(&variant.attrs)?,
        ..Command::default()
    };
    for meta in attrs(&variant.attrs, "command")? {
        match key(&meta)?.as_str() {
            "external_subcommand" if matches!(meta, Meta::Path(_)) => {
                let Fields::Unnamed(fields) = &variant.fields else {
                    return Err("expected an external argument tuple".into());
                };
                if fields.unnamed.len() != 1
                    || type_name(&fields.unnamed[0].ty)? != ("Vec".into(), Some("OsString".into()))
                {
                    return Err("unsupported external argument type".into());
                }
                return Ok(None);
            }
            "name" => command.name = string(value(&meta)?)?,
            "hide" => command.hidden = boolean(value(&meta)?)?,
            "about" => command.help = Some(string(value(&meta)?)?),
            "alias" => command.aliases.push(string(value(&meta)?)?),
            "visible_alias" => command.visible_aliases.push(string(value(&meta)?)?),
            name => return Err(format!("unsupported subcommand setting {name}")),
        }
    }
    let (fields, type_attrs, implicit_group) = match &variant.fields {
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let (name, inner) = type_name(&fields.unnamed[0].ty)?;
            let Some(Item::Struct(args)) = definitions.get(&name) else {
                return Err(format!("unresolved argument struct {name}"));
            };
            if inner.is_some() {
                return Err("generic argument struct".into());
            }
            require_derive(&args.attrs, "Args")?;
            reject_conditional(&args.attrs)?;
            (&args.fields, args.attrs.as_slice(), Some(name))
        }
        Fields::Unnamed(_) => return Err("unsupported tuple subcommand".into()),
        fields @ Fields::Named(_) => (fields, &[][..], Some(variant.ident.to_string())),
        fields @ Fields::Unit => (fields, &[][..], None),
    };
    if command.help.is_none() {
        command.help = help(type_attrs)?;
    }
    for field in fields {
        if let Some(argument) = argument(field, definitions)? {
            command.arguments.push(argument);
        }
    }
    let group_settings = attrs(type_attrs, "group")?;
    match group_settings.as_slice() {
        [] => {
            if let Some(id) = implicit_group.filter(|_| !command.arguments.is_empty()) {
                command.groups.push(Group {
                    id,
                    arguments: command.arguments.iter().map(|arg| arg.id.clone()).collect(),
                    multiple: true,
                    ..Group::default()
                });
            }
        }
        [Meta::Path(path)] if path.is_ident("skip") => {}
        _ => return Err("unsupported argument group settings".into()),
    }
    for meta in attrs(type_attrs, "command")? {
        let Meta::List(list) = meta else {
            return Err("expected command group expression".into());
        };
        if !list.path.is_ident("group") {
            return Err("unsupported command setting on argument struct".into());
        }
        command.groups.push(group(
            list.parse_args().map_err(|error| error.to_string())?,
        )?);
    }
    Ok(Some(command))
}

fn argument(field: &syn::Field, definitions: &Definitions<'_>) -> Result<Option<Argument>, String> {
    reject_conditional(&field.attrs)?;
    if !attrs(&field.attrs, "command")?.is_empty() {
        return Err("unsupported command setting on argument".into());
    }
    let settings = attrs(&field.attrs, "arg")?;
    if matches!(settings.as_slice(), [Meta::Path(path)] if path.is_ident("skip")) {
        return Ok(None);
    }
    let id = field
        .ident
        .as_ref()
        .ok_or("expected named argument")?
        .to_string();
    let (outer, inner) = type_name(&field.ty)?;
    let base = inner.as_deref().unwrap_or(&outer);
    let mut arg = Argument {
        value_name: id.to_ascii_uppercase(),
        id,
        help: help(&field.attrs)?,
        repeated: outer == "Vec",
        ..Argument::default()
    };
    let enumerated = settings
        .iter()
        .any(|meta| meta.path().is_ident("value_enum"));
    arg.kind = match base {
        "bool" if outer == "bool" => None,
        "String" => Some(ValueKind::String),
        "OsString" => Some(ValueKind::OsString),
        "PathBuf" => Some(ValueKind::Path),
        "u32" => Some(ValueKind::U32),
        "u64" => Some(ValueKind::U64),
        "usize" => Some(ValueKind::Usize),
        name if enumerated => {
            let Some(Item::Enum(values)) = definitions.get(name) else {
                return Err(format!("unresolved value enum {name}"));
            };
            require_derive(&values.attrs, "ValueEnum")?;
            reject_conditional(&values.attrs)?;
            if !attrs(&values.attrs, "value")?.is_empty() {
                return Err(format!("unsupported value enum settings on {name}"));
            }
            for variant in &values.variants {
                reject_conditional(&variant.attrs)?;
                if variant.attrs.iter().any(|attr| attr.path().is_ident("doc"))
                    || !matches!(variant.fields, Fields::Unit)
                    || !attrs(&variant.attrs, "value")?.is_empty()
                {
                    return Err(format!("unsupported value variant {}", variant.ident));
                }
                arg.possible_values.push(kebab(&variant.ident.to_string()));
            }
            Some(ValueKind::String)
        }
        name => return Err(format!("unsupported argument type {name}")),
    };
    if inner.is_some() && !matches!(outer.as_str(), "Option" | "Vec") {
        return Err(format!("unsupported argument container {outer}"));
    }
    let mut required = None;
    let mut explicit_variadic = false;
    for meta in settings {
        match key(&meta)?.as_str() {
            "long" => {
                arg.long = Some(if matches!(meta, Meta::Path(_)) {
                    kebab(&arg.id)
                } else {
                    string(value(&meta)?)?
                })
            }
            "short" => {
                arg.short = Some(if matches!(meta, Meta::Path(_)) {
                    arg.id.chars().next().ok_or("empty argument id")?
                } else {
                    match value(&meta)? {
                        Expr::Lit(expr) => match &expr.lit {
                            Lit::Char(character) => character.value(),
                            _ => return Err("expected short option character".into()),
                        },
                        _ => return Err("expected short option character".into()),
                    }
                })
            }
            "value_enum" if matches!(meta, Meta::Path(_)) => {}
            "global" => arg.global = boolean(value(&meta)?)?,
            "last" => arg.last = boolean(value(&meta)?)?,
            "trailing_var_arg" => arg.trailing_var_arg = boolean(value(&meta)?)?,
            "allow_hyphen_values" => arg.allow_hyphen_values = boolean(value(&meta)?)?,
            "num_args" if arg.repeated => {
                let Expr::Range(range) = value(&meta)? else {
                    return Err("only one-or-more variadic argument counts are supported".into());
                };
                if range.end.is_some()
                    || !matches!(range.limits, syn::RangeLimits::HalfOpen(_))
                    || range.start.as_deref().map(integer).transpose()? != Some(1)
                {
                    return Err("only one-or-more variadic argument counts are supported".into());
                }
                explicit_variadic = true;
            }
            "required" => required = Some(boolean(value(&meta)?)?),
            "value_name" => arg.value_name = string(value(&meta)?)?,
            "default_value" => arg.default_value = Some(string(value(&meta)?)?),
            "default_value_t" => arg.default_value = Some(integer(value(&meta)?)?.to_string()),
            "alias" => arg.aliases.push(string(value(&meta)?)?),
            "visible_alias" => arg.visible_aliases.push(string(value(&meta)?)?),
            "requires" => arg.requires.push(string(value(&meta)?)?),
            "requires_all" => arg.requires.extend(strings(value(&meta)?)?),
            "conflicts_with" => arg.conflicts.push(string(value(&meta)?)?),
            "conflicts_with_all" => arg.conflicts.extend(strings(value(&meta)?)?),
            "value_parser" => match value(&meta)? {
                Expr::Array(_) if matches!(arg.kind, Some(ValueKind::String)) => {
                    arg.possible_values = strings(value(&meta)?)?
                }
                expr if matches!(arg.kind, Some(ValueKind::U64)) => {
                    arg.bounds = Some(bounds(expr)?)
                }
                _ => return Err(format!("unsupported parser for {}", arg.id)),
            },
            name => return Err(format!("unsupported argument setting {name} on {}", arg.id)),
        }
    }
    if explicit_variadic && (arg.long.is_some() || arg.short.is_some()) {
        return Err("explicit variadic option counts are unsupported".into());
    }
    arg.required =
        required.unwrap_or(arg.kind.is_some() && inner.is_none() && arg.default_value.is_none());
    Ok(Some(arg))
}

fn reject_conditional(attributes: &[Attribute]) -> Result<(), String> {
    if attributes.iter().any(|attr| {
        attr.path().is_ident("cfg")
            || attr.path().is_ident("cfg_attr")
            || attr.path().is_ident("clap")
            || attr.path().is_ident("structopt")
    }) {
        Err("conditional or legacy derive attributes require review".into())
    } else {
        Ok(())
    }
}

fn attrs(attributes: &[Attribute], name: &str) -> Result<Vec<Meta>, String> {
    let mut result = Vec::new();
    for attribute in attributes.iter().filter(|attr| attr.path().is_ident(name)) {
        result.extend(
            attribute
                .parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                )
                .map_err(|error| error.to_string())?,
        );
    }
    Ok(result)
}

fn require_derive(attributes: &[Attribute], name: &str) -> Result<(), String> {
    if attrs(attributes, "derive")?.iter().any(|meta| {
        meta.path()
            .segments
            .last()
            .is_some_and(|seg| seg.ident == name)
    }) {
        Ok(())
    } else {
        Err(format!("expected {name} derive"))
    }
}

fn key(meta: &Meta) -> Result<String, String> {
    meta.path()
        .get_ident()
        .map(ToString::to_string)
        .ok_or("expected attribute name".into())
}

fn value(meta: &Meta) -> Result<&Expr, String> {
    match meta {
        Meta::NameValue(value) => Ok(&value.value),
        _ => Err(format!("expected literal for {}", key(meta)?)),
    }
}

fn string(expr: &Expr) -> Result<String, String> {
    match expr {
        Expr::Lit(expr) => match &expr.lit {
            Lit::Str(value) => Ok(value.value()),
            _ => Err("expected string".into()),
        },
        _ => Err("expected string".into()),
    }
}

fn strings(expr: &Expr) -> Result<Vec<String>, String> {
    match expr {
        Expr::Array(expr) => expr.elems.iter().map(string).collect(),
        _ => Err("expected array of strings".into()),
    }
}

fn boolean(expr: &Expr) -> Result<bool, String> {
    match expr {
        Expr::Lit(expr) => match &expr.lit {
            Lit::Bool(value) => Ok(value.value),
            _ => Err("expected boolean".into()),
        },
        _ => Err("expected boolean".into()),
    }
}

fn integer(expr: &Expr) -> Result<u64, String> {
    match expr {
        Expr::Lit(expr) => match &expr.lit {
            Lit::Int(value) => value
                .base10_parse::<u64>()
                .map_err(|error| error.to_string()),
            _ => Err("expected nonnegative integer".into()),
        },
        _ => Err("expected nonnegative integer".into()),
    }
}

fn type_name(ty: &Type) -> Result<(String, Option<String>), String> {
    let Type::Path(path) = ty else {
        return Err("expected named argument type".into());
    };
    if path.qself.is_some() {
        return Err("unsupported qualified argument type".into());
    }
    let segment = path.path.segments.last().ok_or("empty type path")?;
    let inner = match &segment.arguments {
        PathArguments::None => None,
        PathArguments::AngleBracketed(parameters) if parameters.args.len() == 1 => {
            let Some(GenericArgument::Type(inner)) = parameters.args.first() else {
                return Err("expected generic type".into());
            };
            let (name, nested) = type_name(inner)?;
            if nested.is_some() {
                return Err("nested argument container".into());
            }
            Some(name)
        }
        _ => return Err("unsupported argument type parameters".into()),
    };
    Ok((segment.ident.to_string(), inner))
}

fn bounds(expr: &Expr) -> Result<Bounds, String> {
    let Expr::MethodCall(call) = expr else {
        return Err("expected typed numeric range".into());
    };
    let Expr::Macro(parser) = call.receiver.as_ref() else {
        return Err("expected value_parser! macro".into());
    };
    let provider = parser
        .mac
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    let ty = parser
        .mac
        .parse_body::<Type>()
        .map_err(|error| error.to_string())?;
    if call.method != "range"
        || call.args.len() != 1
        || provider != ["clap", "value_parser"]
        || type_name(&ty)? != ("u64".into(), None)
    {
        return Err("unsupported numeric parser".into());
    }
    let Some(Expr::Range(range)) = call.args.first() else {
        return Err("expected literal numeric bounds".into());
    };
    let minimum = range
        .start
        .as_deref()
        .map(integer)
        .transpose()?
        .unwrap_or(0);
    let maximum = range.end.as_deref().map(integer).transpose()?;
    let maximum = match (&range.limits, maximum) {
        (syn::RangeLimits::HalfOpen(_), Some(end)) => {
            Some(end.checked_sub(1).ok_or("empty numeric range")?)
        }
        (_, end) => end,
    };
    Ok(Bounds { minimum, maximum })
}

fn group(expr: Expr) -> Result<Group, String> {
    match expr {
        Expr::MethodCall(call) if call.args.len() == 1 => {
            let mut group = group(*call.receiver)?;
            let arg = call.args.first().ok_or("missing group argument")?;
            match call.method.to_string().as_str() {
                "required" => group.required = boolean(arg)?,
                "multiple" => group.multiple = boolean(arg)?,
                "args" => group.arguments = strings(arg)?,
                _ => return Err(format!("unsupported group setting {}", call.method)),
            }
            Ok(group)
        }
        Expr::Call(call) if call.args.len() == 1 => {
            let Expr::Path(path) = call.func.as_ref() else {
                return Err("expected group constructor".into());
            };
            let parts = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>();
            if parts != ["clap", "ArgGroup", "new"] {
                return Err("unsupported group constructor".into());
            }
            Ok(Group {
                id: string(call.args.first().ok_or("group id missing")?)?,
                ..Group::default()
            })
        }
        _ => Err("unsupported argument group expression".into()),
    }
}

fn help(attributes: &[Attribute]) -> Result<Option<String>, String> {
    let mut lines = Vec::new();
    for attribute in attributes.iter().filter(|attr| attr.path().is_ident("doc")) {
        let line = string(value(&attribute.meta)?)?;
        let line = line.trim();
        if line.is_empty() && !lines.is_empty() {
            return Err("multi-paragraph help requires review".into());
        } else if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }
    let result = lines.join(" ");
    if result.is_empty() {
        return Ok(None);
    }
    Ok(Some(if result.ends_with("..") {
        result
    } else {
        result.strip_suffix('.').unwrap_or(&result).into()
    }))
}

fn kebab(name: &str) -> String {
    let mut result = String::new();
    let chars = name.chars().collect::<Vec<_>>();
    for (index, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            if !result.is_empty() && !result.ends_with('-') {
                result.push('-');
            }
            continue;
        }
        if let Some(&previous) = index.checked_sub(1).and_then(|index| chars.get(index)) {
            let next = chars.get(index + 1);
            if ch.is_uppercase()
                && (previous.is_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_uppercase() && next.is_some_and(|ch| ch.is_lowercase())))
            {
                result.push('-');
            }
        }
        result.extend(ch.to_lowercase());
    }
    result.trim_end_matches('-').into()
}
