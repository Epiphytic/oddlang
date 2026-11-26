use super::schema::{Access, Expr, Graph, Intrinsic, Limit, Operation, RustOverlay, State, Ty};
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
    let bounds = [
        (
            "byte_capacity",
            graph.size(&state.byte_capacity).unwrap().to_string(),
        ),
        (
            "record_capacity",
            graph.size(&state.record_capacity).unwrap().to_string(),
        ),
        (
            "append_limit",
            graph.size(&state.append_limit).unwrap().to_string(),
        ),
        (
            "cursor_limit",
            graph.cursor(&state.cursor_limit).unwrap().to_string(),
        ),
        (
            "receipt_capacity",
            graph.size(&state.receipt_capacity).unwrap().to_string(),
        ),
    ];
    writeln!(
        destination,
        "\npub struct {ty} {{\n    journal: __oddlang_journal::Store,\n}}\n\nimpl Default for {ty} {{\n    fn default() -> Self {{\n        Self {{ journal: __oddlang_journal::Store::new(__oddlang_journal::Bounds {{"
    )
    .unwrap();
    for (key, value) in bounds {
        writeln!(destination, "            {key}: {value},").unwrap();
    }
    writeln!(destination, "        }}, {initial}) }}\n    }}\n}}\n\n#[allow(unused_parens, unused_variables)]\nimpl {ty} {{").unwrap();
    for operation in graph
        .operations
        .iter()
        .filter(|operation| operation.state == state.id)
    {
        let renderer = Method {
            graph,
            binding,
            operation,
            named: BTreeMap::new(),
            definitions: Vec::new(),
        };
        renderer.write(destination);
    }
    destination.push_str("}\n");
}

struct Method<'a> {
    graph: &'a Graph,
    binding: &'a RustOverlay,
    operation: &'a Operation,
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
                    writeln!(rules, "{indent}let mut staged = self.journal.clone();").unwrap();
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
                    writeln!(
                        rules,
                        "{indent}self.journal = staged;\n{indent}return outcome;"
                    )
                    .unwrap();
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
            Expr::Fact { fact } => format!("self.journal.{}()", fact.method()),
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
        Advance => format!("self.journal.advance({}, {})", at(0), at(1)),
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
            "std::borrow::Cow::<'_, [u8]>::Owned(self.journal.slice({}, {}))",
            at(0),
            at(1)
        ),
        HasReceipt => format!("self.journal.has_receipt({})", at(0)),
        SameReceipt => format!(
            "self.journal.same_receipt({}, {}, ({}).as_ref())",
            at(0),
            at(1),
            at(2)
        ),
        FrontReceiptEvictable => "self.journal.front_receipt_evictable()".to_owned(),
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
