use super::schema::{
    Access, Expr, Graph, Intrinsic, Limit, Operation, RustOverlay, State, Storage, Ty,
};
use std::{collections::BTreeMap, fmt::Write};

pub(super) fn rust(graph: &Graph, binding: &RustOverlay) -> String {
    let mut out = "#[allow(dead_code)]\nmod __oddlang_journal {\n".to_owned();
    for line in include_str!("native.rs").lines() {
        if !line.is_empty() {
            out.push_str("    ");
            out.push_str(line);
        }
        out.push('\n');
    }
    out.push_str("}\n");
    for state in &graph.states {
        write_state(&mut out, graph, binding, state);
    }
    out
}

fn write_state(destination: &mut String, graph: &Graph, binding: &RustOverlay, state: &State) {
    let ty = &binding.types[&state.id];
    let initial = state.initial_cursor.0;
    let bounds = render_bounds(graph, state);
    let storage = binding.storage.get(&state.id);
    if let Some(storage) = storage {
        write_flat_state(destination, ty, initial, &bounds, storage);
    } else {
        writeln!(
            destination,
            "\npub struct {ty} {{\n    journal: __oddlang_journal::Store,\n}}"
        )
        .unwrap();
        writeln!(destination,
            "\nimpl Default for {ty} {{\n    fn default() -> Self {{\n        Self {{ journal: __oddlang_journal::Store::new({bounds}, {initial}) }}\n    }}\n}}",
        ).unwrap();
    }
    writeln!(
        destination,
        "\n#[allow(unused_parens, unused_variables)]\nimpl {ty} {{"
    )
    .unwrap();
    for operation in graph
        .operations
        .iter()
        .filter(|operation| operation.state == state.id)
    {
        let renderer = Method {
            graph,
            binding,
            operation,
            storage,
            named: BTreeMap::new(),
            definitions: Vec::new(),
        };
        renderer.write(destination);
    }
    destination.push_str("}\n");
}

fn render_bounds(graph: &Graph, state: &State) -> String {
    let byte_capacity = graph.size(&state.byte_capacity).unwrap();
    let record_capacity = graph.size(&state.record_capacity).unwrap();
    let append_limit = graph.size(&state.append_limit).unwrap();
    let cursor_limit = graph.cursor(&state.cursor_limit).unwrap();
    let receipt_capacity = graph.size(&state.receipt_capacity).unwrap();
    format!(
        "__oddlang_journal::Bounds {{ byte_capacity: {byte_capacity}, record_capacity: {record_capacity}, append_limit: {append_limit}, cursor_limit: {cursor_limit}, receipt_capacity: {receipt_capacity} }}"
    )
}

fn write_flat_state(output: &mut String, ty: &str, initial: u64, bounds: &str, storage: &Storage) {
    let [start, end, final_cursor, records, receipts] = storage.fields();
    writeln!(output,
        "\npub struct {ty} {{\n    {start}: u64,\n    {end}: u64,\n    {final_cursor}: Option<u64>,\n    {records}: std::collections::VecDeque<__oddlang_journal::Record>,\n    {receipts}: std::collections::VecDeque<__oddlang_journal::Receipt>,\n}}"
    ).unwrap();
    writeln!(
        output,
        "\nimpl Default for {ty} {{\n    fn default() -> Self {{\n        Self {{ {start}: {initial}, {end}: {initial}, {final_cursor}: None, {records}: Default::default(), {receipts}: Default::default() }}\n    }}\n}}"
    ).unwrap();
    writeln!(
        output,
        "\nimpl {ty} {{\n    fn __oddlang_view(&self) -> __oddlang_journal::View<'_> {{\n        __oddlang_journal::View::new({bounds}, self.{start}, self.{end}, self.{final_cursor}, &self.{records}, &self.{receipts})\n    }}\n}}"
    ).unwrap();
}

struct Method<'a> {
    graph: &'a Graph,
    binding: &'a RustOverlay,
    operation: &'a Operation,
    storage: Option<&'a Storage>,
    named: BTreeMap<String, usize>,
    definitions: Vec<String>,
}

impl Method<'_> {
    fn write(mut self, output: &mut String) {
        let op = self.operation;
        let name = &self.binding.methods[&op.id];
        let receiver = match op.access {
            Access::Read => "&self",
            Access::Write => "&mut self",
        };
        let mut parameters = receiver.to_owned();
        for (i, param) in op.params.iter().enumerate() {
            write!(parameters, ", input_{i}: {}", rust_ty(param.ty, true)).unwrap();
        }
        let native = rust_ty(op.result, false);
        let output_type = if op.fallible {
            format!("Result<{native}, {}>", self.binding.error_type)
        } else {
            native.to_owned()
        };
        writeln!(
            output,
            "    pub fn {name}({parameters}) -> {output_type} {{"
        )
        .unwrap();

        let view = if self.storage.is_some() {
            "self.__oddlang_view()"
        } else {
            "self.journal.view()"
        };
        writeln!(output, "        let journal = {view};").unwrap();
        let mut rules = String::new();
        for case in &op.cases {
            let conditional = !case.when.is_empty();
            let indent = if conditional {
                "            "
            } else {
                "        "
            };
            if conditional {
                let predicates = case
                    .when
                    .iter()
                    .map(|condition| self.expr(condition))
                    .collect::<Vec<_>>()
                    .join(" && ");
                writeln!(rules, "        if {predicates} {{").unwrap();
            }
            if let Some(error) = &case.reject {
                writeln!(rules, "{indent}return Err({});", self.error(error)).unwrap();
            } else {
                let mut value = self.expr(case.value.as_ref().expect("validated outcome"));
                if op.result == Ty::Bytes {
                    value = format!("({value}).into_owned()");
                }
                if op.fallible {
                    value = format!("Ok({value})");
                }
                if case.effects.is_empty() {
                    writeln!(rules, "{indent}return {value};").unwrap();
                } else {
                    writeln!(rules, "{indent}let outcome = {value};").unwrap();
                    writeln!(rules, "{indent}let mut staged = journal.stage();").unwrap();
                    for effect in &case.effects {
                        let (method, error, operands) = effect.parts();
                        let args = operands
                            .into_iter()
                            .map(|(expr, ty)| {
                                let value = self.expr(expr);
                                if ty == Ty::Bytes {
                                    format!("({value}).as_ref()")
                                } else {
                                    value
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        let error = self.error(error);
                        writeln!(
                            rules,
                            "{indent}if !staged.{method}({args}) {{ return Err({error}); }}"
                        )
                        .unwrap();
                    }
                    if let Some(storage) = self.storage {
                        let [start, end, final_cursor, records, receipts] = storage.fields();
                        writeln!(rules,
                            "{indent}(self.{start}, self.{end}, self.{final_cursor}, self.{records}, self.{receipts}) = staged.into_parts();"
                        ).unwrap();
                    } else {
                        writeln!(rules, "{indent}self.journal = staged;").unwrap();
                    }
                    writeln!(rules, "{indent}return outcome;").unwrap();
                }
            }
            if conditional {
                rules.push_str("        }\n");
            }
        }
        for (i, definition) in self.definitions.iter().enumerate() {
            writeln!(output, "        let expression_{i} = || {definition};").unwrap();
        }
        output.push_str(&rules);
        output.push_str("    }\n\n");
    }

    fn error(&self, id: &str) -> String {
        format!(
            "{}::{}",
            self.binding.error_type, self.binding.error_variants[id]
        )
    }

    fn expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Arg { arg } => {
                let i = self
                    .operation
                    .params
                    .iter()
                    .position(|parameter| parameter.name == *arg)
                    .expect("validated argument");
                if self.operation.params[i].ty == Ty::Bytes {
                    format!("std::borrow::Cow::<'_, [u8]>::Borrowed(input_{i})")
                } else {
                    format!("input_{i}")
                }
            }
            Expr::Limit { limit } => match self.graph.limits[limit] {
                Limit::Cursor { cursor } => format!("{}u64", cursor.0),
                Limit::Size { size } => format!("{size}usize"),
            },
            Expr::Fact { fact } => format!("journal.{}()", fact.method()),
            Expr::Cursor { cursor } => format!("{}u64", cursor.0),
            Expr::Size { size } => format!("{size}usize"),
            Expr::Flag { flag } => flag.to_string(),
            Expr::None { .. } => "None::<u64>".to_owned(),
            Expr::Unit { .. } => "()".to_owned(),
            Expr::Ref { expr } => {
                let i = if let Some(i) = self.named.get(expr) {
                    *i
                } else {
                    let code = self.expr(&self.graph.expressions[expr]);
                    let i = self.definitions.len();
                    self.definitions.push(code);
                    self.named.insert(expr.clone(), i);
                    i
                };
                format!("expression_{i}()")
            }
            Expr::Apply { op, args } => {
                let operands = args
                    .iter()
                    .map(|operand| self.expr(operand))
                    .collect::<Vec<_>>();
                apply(*op, &operands)
            }
        }
    }
}

fn apply(op: Intrinsic, args: &[String]) -> String {
    use Intrinsic::*;
    let at = |index: usize| &args[index];
    match op {
        SizeOf => format!("({}).len()", at(0)),
        Advance => format!("journal.advance({}, {})", at(0), at(1)),
        Some => format!("Some({})", at(0)),
        Exists => format!("({}).is_some()", at(0)),
        Equal => format!("({} == {})", at(0), at(1)),
        Less => format!("({} < {})", at(0), at(1)),
        Greater => format!("({} > {})", at(0), at(1)),
        And => format!("({})", args.join(" && ")),
        Or => format!("({})", args.join(" || ")),
        Not => format!("!({})", at(0)),
        Choose => format!("(if {} {{ {} }} else {{ {} }})", at(0), at(1), at(2)),
        Slice => format!(
            "std::borrow::Cow::<'_, [u8]>::Owned(journal.slice({}, {}))",
            at(0),
            at(1)
        ),
        HasReceipt => format!("journal.has_receipt({})", at(0)),
        SameReceipt => format!(
            "journal.same_receipt({}, {}, ({}).as_ref())",
            at(0),
            at(1),
            at(2)
        ),
        FrontReceiptEvictable => "journal.front_receipt_evictable()".to_owned(),
    }
}

fn rust_ty(ty: Ty, parameter: bool) -> &'static str {
    match ty {
        Ty::Cursor => "u64",
        Ty::Size => "usize",
        Ty::Flag => "bool",
        Ty::OptionalCursor => "Option<u64>",
        Ty::Bytes if parameter => "&[u8]",
        Ty::Bytes => "Vec<u8>",
        Ty::Unit => "()",
    }
}
