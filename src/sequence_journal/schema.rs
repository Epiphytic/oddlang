use crate::shared_declarations::unique_map;
use oxigraph::model::NamedNode;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const FORMAT: &str = "oddlang-sequence-journal-v1";
pub(super) const RUST_FORMAT: &str = "oddlang-sequence-journal-rust-v1";
pub(super) type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Ty {
    Cursor,
    Size,
    Flag,
    OptionalCursor,
    Bytes,
    Unit,
}

#[derive(Clone, Copy)]
pub(super) struct Cursor(pub u64);

impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Number(u64),
            Text(String),
        }
        match Wire::deserialize(deserializer)? {
            Wire::Number(value) => Ok(Self(value)),
            Wire::Text(text) => {
                let value = text.parse::<u64>().map_err(serde::de::Error::custom)?;
                if value.to_string() != text {
                    return Err(serde::de::Error::custom("noncanonical cursor"));
                }
                Ok(Self(value))
            }
        }
    }
}

impl Serialize for Cursor {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum Limit {
    Cursor { cursor: Cursor },
    Size { size: u32 },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Graph {
    pub format: String,
    #[serde(deserialize_with = "unique_map")]
    pub limits: BTreeMap<String, Limit>,
    pub errors: Vec<String>,
    pub states: Vec<State>,
    pub operations: Vec<Operation>,
    #[serde(default, deserialize_with = "unique_map")]
    pub expressions: BTreeMap<String, Expr>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct State {
    pub id: String,
    pub byte_capacity: String,
    pub record_capacity: String,
    pub cursor_limit: String,
    pub append_limit: String,
    pub receipt_capacity: String,
    pub initial_cursor: Cursor,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Access {
    Read,
    Write,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Operation {
    pub id: String,
    pub state: String,
    pub access: Access,
    pub params: Vec<Param>,
    pub result: Ty,
    pub fallible: bool,
    pub cases: Vec<Case>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Param {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Ty,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Case {
    pub id: String,
    pub when: Vec<Expr>,
    pub reject: Option<String>,
    pub value: Option<Expr>,
    #[serde(default)]
    pub effects: Vec<Effect>,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum Expr {
    Arg { arg: String },
    Limit { limit: String },
    Fact { fact: Fact },
    Cursor { cursor: Cursor },
    Size { size: u32 },
    Flag { flag: bool },
    None { none: bool },
    Unit { unit: bool },
    Ref { expr: String },
    Apply { op: Intrinsic, args: Vec<Expr> },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Fact {
    Start,
    End,
    Final,
    Buffered,
    ByteCredit,
    RecordCount,
    RecordCredit,
    ReceiptCount,
    ReceiptCredit,
    FrontReceiptEnd,
    LastReceiptStart,
    LastReceiptEnd,
}

impl Fact {
    pub fn ty(self) -> Ty {
        match self {
            Self::Start | Self::End => Ty::Cursor,
            Self::Final | Self::FrontReceiptEnd | Self::LastReceiptStart | Self::LastReceiptEnd => {
                Ty::OptionalCursor
            }
            _ => Ty::Size,
        }
    }

    pub fn method(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
            Self::Final => "final_cursor",
            Self::Buffered => "buffered",
            Self::ByteCredit => "byte_credit",
            Self::RecordCount => "record_count",
            Self::RecordCredit => "record_credit",
            Self::ReceiptCount => "receipt_count",
            Self::ReceiptCredit => "receipt_credit",
            Self::FrontReceiptEnd => "front_receipt_end",
            Self::LastReceiptStart => "last_receipt_start",
            Self::LastReceiptEnd => "last_receipt_end",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Intrinsic {
    SizeOf,
    Advance,
    Some,
    Exists,
    Equal,
    Less,
    Greater,
    And,
    Or,
    Not,
    Choose,
    Slice,
    HasReceipt,
    SameReceipt,
    FrontReceiptEvictable,
}

impl Intrinsic {
    fn infer(self, args: &[Ty]) -> Result<Ty> {
        use Intrinsic::*;
        use Ty::*;
        let result = match (self, args) {
            (SizeOf, [Bytes]) => Size,
            (Advance, [Cursor, Size]) | (Some, [Cursor]) => OptionalCursor,
            (Exists, [OptionalCursor]) => Flag,
            (Equal, [left, right]) if left == right && !matches!(left, Bytes | Unit) => Flag,
            (Less | Greater, [Cursor, Cursor] | [Size, Size]) => Flag,
            (And | Or, [Flag, Flag, rest @ ..]) if rest.iter().all(|value| *value == Flag) => Flag,
            (Not, [Flag]) => Flag,
            (Choose, [Flag, left, right]) if left == right => *left,
            (Slice, [Cursor, Size]) => Bytes,
            (HasReceipt, [Cursor])
            | (SameReceipt, [Cursor, OptionalCursor, Bytes])
            | (FrontReceiptEvictable, []) => Flag,
            _ => return Err(format!("invalid operands for {self:?}: {args:?}")),
        };
        Ok(result)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Effect {
    Append {
        bytes: Expr,
        error: String,
    },
    Trim {
        cursor: Expr,
        error: String,
    },
    Finish {
        cursor: Expr,
        error: String,
    },
    EvictReceipt {
        error: String,
    },
    Remember {
        cursor: Expr,
        bytes: Expr,
        error: String,
    },
}

impl Effect {
    pub fn parts(&self) -> (&'static str, &str, Vec<(&Expr, Ty)>) {
        match self {
            Self::Append { bytes, error } => ("append", error, vec![(bytes, Ty::Bytes)]),
            Self::Trim { cursor, error } => ("trim", error, vec![(cursor, Ty::Cursor)]),
            Self::Finish { cursor, error } => ("finish", error, vec![(cursor, Ty::Cursor)]),
            Self::EvictReceipt { error } => ("evict_receipt", error, vec![]),
            Self::Remember {
                cursor,
                bytes,
                error,
            } => (
                "remember",
                error,
                vec![(cursor, Ty::Cursor), (bytes, Ty::Bytes)],
            ),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RustOverlay {
    pub format: String,
    #[serde(deserialize_with = "unique_map")]
    pub types: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    pub methods: BTreeMap<String, String>,
    pub error_type: String,
    #[serde(deserialize_with = "unique_map")]
    pub error_variants: BTreeMap<String, String>,
}

fn identifier(value: &str) -> Result<()> {
    NamedNode::new(value)
        .map(|_| ())
        .map_err(|_| format!("invalid journal identity {value:?}"))
}

fn key(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(format!("invalid journal name {value:?}"));
    }
    Ok(())
}

fn unique_ids<'a>(items: impl IntoIterator<Item = &'a str>) -> Result<BTreeSet<&'a str>> {
    let mut output = BTreeSet::new();
    for item in items {
        identifier(item)?;
        if !output.insert(item) {
            return Err(format!("duplicate journal identity {item:?}"));
        }
    }
    Ok(output)
}

fn rust_ident(value: &str) -> Result<()> {
    if value == "gen"
        || syn::parse_str::<syn::Ident>(value)
            .ok()
            .is_none_or(|parsed| parsed.to_string() != value)
    {
        return Err(format!("invalid journal Rust name {value:?}"));
    }
    Ok(())
}

impl RustOverlay {
    pub fn validate(&self) -> Result<()> {
        if self.format != RUST_FORMAT {
            return Err("unsupported sequence journal Rust overlay".into());
        }
        for binding in [&self.types, &self.methods, &self.error_variants] {
            for (id, name) in binding {
                identifier(id)?;
                rust_ident(name)?;
            }
        }
        let path = syn::parse_str::<syn::Path>(&self.error_type)
            .map_err(|_| "invalid external journal Rust error path".to_owned())?;
        if path
            .segments
            .iter()
            .any(|segment| !segment.arguments.is_none())
        {
            return Err("external journal Rust error must be a simple path".into());
        }
        let names = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        let leading = if path.leading_colon.is_some() {
            "::"
        } else {
            ""
        };
        if self.error_type != format!("{leading}{}", names.join("::")) {
            return Err("external journal Rust error must use a canonical path".into());
        }
        Ok(())
    }

    pub fn validate_bindings(&self, graph: &Graph) -> Result<()> {
        if self
            .types
            .keys()
            .ne(graph.states.iter().map(|state| &state.id))
            || self
                .methods
                .keys()
                .ne(graph.operations.iter().map(|operation| &operation.id))
            || self.error_variants.keys().ne(graph.errors.iter())
        {
            return Err("Rust binding must cover every journal state, operation and error".into());
        }
        let canon = |name: &String| name.trim_start_matches("r#").to_owned();
        let types: BTreeSet<_> = self.types.values().map(canon).collect();
        if types.len() != self.types.len() || types.contains("__oddlang_journal") {
            return Err("duplicate or reserved journal Rust type".into());
        }
        let mut methods = BTreeSet::new();
        for operation in &graph.operations {
            let method = canon(&self.methods[&operation.id]);
            if method == "default" || !methods.insert((&operation.state, method)) {
                return Err("duplicate or reserved journal Rust method".into());
            }
        }
        Ok(())
    }
}

impl Graph {
    pub fn size(&self, name: &str) -> Result<u32> {
        match self.limits.get(name) {
            Some(Limit::Size { size }) => Ok(*size),
            _ => Err(format!("unknown size limit {name:?}")),
        }
    }

    pub fn cursor(&self, name: &str) -> Result<u64> {
        match self.limits.get(name) {
            Some(Limit::Cursor { cursor }) => Ok(cursor.0),
            _ => Err(format!("unknown cursor limit {name:?}")),
        }
    }

    pub fn validate(mut self) -> Result<Self> {
        if self.format != FORMAT {
            return Err("unsupported sequence journal graph".into());
        }
        if self.states.is_empty() || self.operations.is_empty() {
            return Err("a journal graph needs a state and an operation".into());
        }
        for name in self.limits.keys().chain(self.expressions.keys()) {
            key(name)?;
        }
        self.validate_named_expressions()?;
        let states = unique_ids(self.states.iter().map(|state| state.id.as_str()))?;
        let errors = unique_ids(self.errors.iter().map(String::as_str))?;
        unique_ids(
            self.operations
                .iter()
                .map(|operation| operation.id.as_str()),
        )?;
        unique_ids(
            self.operations
                .iter()
                .flat_map(|operation| operation.cases.iter().map(|case| case.id.as_str())),
        )?;
        for state in &self.states {
            let capacity = self.size(&state.byte_capacity)?;
            let records = self.size(&state.record_capacity)?;
            let append = self.size(&state.append_limit)?;
            self.size(&state.receipt_capacity)?;
            let limit = self.cursor(&state.cursor_limit)?;
            if capacity == 0
                || records == 0
                || append == 0
                || append > capacity
                || state.initial_cursor.0 > limit
            {
                return Err(format!("invalid journal bounds for {}", state.id));
            }
        }
        for operation in &self.operations {
            if !states.contains(operation.state.as_str()) {
                return Err(format!("unknown state for {}", operation.id));
            }
            self.validate_operation(operation, &errors)
                .map_err(|error| format!("{}: {error}", operation.id))?;
        }
        self.states.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        self.operations.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        self.errors.sort_unstable();
        Ok(self)
    }

    fn validate_named_expressions(&self) -> Result<()> {
        fn visit<'a>(
            graph: &'a Graph,
            name: &'a str,
            active: &mut BTreeSet<&'a str>,
            done: &mut BTreeSet<&'a str>,
        ) -> Result<()> {
            if done.contains(name) {
                return Ok(());
            }
            if !active.insert(name) {
                return Err(format!("named expression cycle at {name:?}"));
            }
            let expr = graph
                .expressions
                .get(name)
                .ok_or_else(|| format!("unknown named expression {name:?}"))?;
            walk(graph, expr, active, done)?;
            active.remove(name);
            done.insert(name);
            Ok(())
        }
        fn walk<'a>(
            graph: &'a Graph,
            expr: &'a Expr,
            active: &mut BTreeSet<&'a str>,
            done: &mut BTreeSet<&'a str>,
        ) -> Result<()> {
            match expr {
                Expr::Ref { expr } => visit(graph, expr, active, done)?,
                Expr::Apply { args, .. } => {
                    for arg in args {
                        walk(graph, arg, active, done)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        let mut done = BTreeSet::new();
        for name in self.expressions.keys() {
            visit(self, name, &mut BTreeSet::new(), &mut done)?;
        }
        Ok(())
    }

    fn validate_operation(&self, operation: &Operation, errors: &BTreeSet<&str>) -> Result<()> {
        let mut params = BTreeMap::new();
        for parameter in &operation.params {
            key(&parameter.name)?;
            if params
                .insert(parameter.name.as_str(), parameter.ty)
                .is_some()
            {
                return Err(format!("duplicate parameter {}", parameter.name));
            }
        }
        let mut types = Inference {
            graph: self,
            params,
            named: BTreeMap::new(),
        };
        let check_error = |id: &str| {
            if !errors.contains(id) {
                return Err(format!("undeclared journal error {id:?}"));
            }
            Ok(())
        };
        if operation.cases.is_empty() {
            return Err("no journal cases".into());
        }
        for (index, case) in operation.cases.iter().enumerate() {
            if case.when.is_empty() != (index + 1 == operation.cases.len()) {
                return Err("exactly the final case must be unconditional".into());
            }
            for condition in &case.when {
                types.check(condition, Ty::Flag)?;
            }
            match (&case.reject, &case.value) {
                (Some(error), None) if operation.fallible && case.effects.is_empty() => {
                    check_error(error)?;
                }
                (None, Some(value)) => {
                    types.check(value, operation.result)?;
                    if !case.effects.is_empty()
                        && (operation.access != Access::Write || !operation.fallible)
                    {
                        return Err("only fallible write operations may apply effects".into());
                    }
                    for effect in &case.effects {
                        let (_, error, operands) = effect.parts();
                        check_error(error)?;
                        for (expr, ty) in operands {
                            types.check(expr, ty)?;
                        }
                    }
                }
                _ => return Err("invalid journal case outcome".into()),
            }
        }
        Ok(())
    }
}

struct Inference<'a> {
    graph: &'a Graph,
    params: BTreeMap<&'a str, Ty>,
    named: BTreeMap<String, Ty>,
}

impl Inference<'_> {
    fn check(&mut self, expr: &Expr, expected: Ty) -> Result<()> {
        let actual = self.infer(expr)?;
        if actual != expected {
            return Err(format!("expected {expected:?}, found {actual:?}"));
        }
        Ok(())
    }

    fn infer(&mut self, expr: &Expr) -> Result<Ty> {
        Ok(match expr {
            Expr::Arg { arg } => *self
                .params
                .get(arg.as_str())
                .ok_or_else(|| format!("unknown argument {arg:?}"))?,
            Expr::Limit { limit } => match self.graph.limits.get(limit) {
                Some(Limit::Size { .. }) => Ty::Size,
                Some(Limit::Cursor { .. }) => Ty::Cursor,
                None => return Err(format!("unknown named limit {limit:?}")),
            },
            Expr::Fact { fact } => fact.ty(),
            Expr::Cursor { .. } => Ty::Cursor,
            Expr::Size { .. } => Ty::Size,
            Expr::Flag { .. } => Ty::Flag,
            Expr::None { none: true } => Ty::OptionalCursor,
            Expr::Unit { unit: true } => Ty::Unit,
            Expr::None { .. } | Expr::Unit { .. } => {
                return Err("invalid journal unit or none literal".into());
            }
            Expr::Ref { expr } => {
                if let Some(ty) = self.named.get(expr) {
                    return Ok(*ty);
                }
                let value = self
                    .graph
                    .expressions
                    .get(expr)
                    .ok_or_else(|| format!("unknown named expression {expr:?}"))?;
                let ty = self.infer(value)?;
                self.named.insert(expr.clone(), ty);
                ty
            }
            Expr::Apply { op, args } => {
                let args = args
                    .iter()
                    .map(|arg| self.infer(arg))
                    .collect::<Result<Vec<_>>>()?;
                op.infer(&args)?
            }
        })
    }
}
