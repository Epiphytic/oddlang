use super::{Binding, Case, Encoding, External, Form, Graph, Predicate};
use std::fmt::Write as _;

fn fault(binding: &Binding, id: &str) -> String {
    let fault = &binding.errors[id];
    let ident = format!("{}::{}", binding.error_type, fault.variant);
    fault
        .argument
        .as_ref()
        .map_or(ident.clone(), |arg| format!("{ident}({arg:?})"))
}

fn field_type(binding: &Binding, encoding: &Encoding) -> String {
    match encoding {
        Encoding::Unsigned { bits, .. } => format!("::core::primitive::u{bits}"),
        Encoding::Fixed { size } => format!("[::core::primitive::u8; {size}]"),
        Encoding::Rest { .. } => "::std::vec::Vec<::core::primitive::u8>".into(),
        Encoding::Enum { id, .. } | Encoding::Fragment { id } => binding.types[id].path.clone(),
        Encoding::Literal { .. } => unreachable!("literal carries no field"),
    }
}

fn pattern(
    case: &Case,
    binding: &Binding,
    name: &str,
    expression: impl Fn(usize) -> String,
) -> String {
    let fields = case
        .slots
        .iter()
        .enumerate()
        .filter(|(_, slot)| slot.encoding.field());
    match case.form {
        Form::Tuple => format!(
            "Self::{name}({})",
            fields
                .map(|(i, _)| expression(i))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Form::Named => format!(
            "Self::{name}{{{}}}",
            fields
                .map(|(i, slot)| format!("{}:{}", binding.fields[&slot.id], expression(i)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn guard_text(case: &Case, predicate: &Predicate, borrowed: bool) -> String {
    let index = |id: &str| {
        case.slots
            .iter()
            .position(|slot| slot.id == id)
            .expect("validated slot")
    };
    let scalar = |id: &str| format!("{}__field_{}", if borrowed { "*" } else { "" }, index(id));
    let variable = |id: &str| format!("__field_{}", index(id));
    match predicate {
        Predicate::Range { field } => {
            let Encoding::Unsigned { maximum, .. } = case.slots[index(field)].encoding else {
                unreachable!()
            };
            format!("{} <= {maximum}", scalar(field))
        }
        Predicate::Count { field } => {
            let Encoding::Rest {
                minimum, maximum, ..
            } = case.slots[index(field)].encoding
            else {
                unreachable!()
            };
            format!(
                "({minimum}..={maximum}).contains(&{}.len())",
                variable(field)
            )
        }
        Predicate::AtMost { left, right } => format!("{} <= {}", scalar(left), scalar(right)),
        Predicate::EndAtMost {
            offset,
            bytes,
            maximum,
        } => {
            format!(
                "({}).checked_add({}.len() as u64).is_some_and(|end| end <= {maximum})",
                scalar(offset),
                variable(bytes)
            )
        }
    }
}

pub(super) fn render(graph: &Graph, binding: &Binding) -> String {
    let mut out = String::new();
    let name = &binding.name;
    let err = &binding.error_type;
    let maximum = graph.maximum;
    writeln!(out, "#[derive(Clone, PartialEq, Eq)]\npub enum {name} {{").unwrap();
    for (id, case) in &graph.cases {
        let name = &binding.cases[id];
        let types = case
            .slots
            .iter()
            .filter(|slot| slot.encoding.field())
            .map(|slot| {
                let ty = field_type(binding, &slot.encoding);
                match case.form {
                    Form::Tuple => ty,
                    Form::Named => format!("{}: {ty}", binding.fields[&slot.id]),
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        match case.form {
            Form::Tuple => writeln!(out, "    {name}({types}),").unwrap(),
            Form::Named => writeln!(out, "    {name} {{ {types} }},").unwrap(),
        }
    }
    writeln!(out,"}}\nimpl {name} {{\n    pub fn encode(&self) -> ::core::result::Result<::std::vec::Vec<u8>,{err}> {{\n        let mut __wire = ::std::vec::Vec::new();\n        let __tag = match self {{").unwrap();
    for (id, case) in &graph.cases {
        let name = &binding.cases[id];
        let arm = pattern(case, binding, name, |i| format!("__field_{i}"));
        writeln!(out, "            {arm} => {{").unwrap();
        for guard in &case.encode_guards {
            writeln!(
                out,
                "                if !({}) {{ return ::core::result::Result::Err({}); }}",
                guard_text(case, &guard.require, true),
                fault(binding, &guard.error)
            )
            .unwrap();
        }
        for (i, slot) in case.slots.iter().enumerate() {
            let var = format!("__field_{i}");
            match &slot.encoding {
                Encoding::Unsigned { .. } => writeln!(
                    out,
                    "                __wire.extend_from_slice(&{var}.to_be_bytes());"
                )
                .unwrap(),
                Encoding::Fixed { .. } | Encoding::Rest { .. } => {
                    writeln!(out, "                __wire.extend_from_slice({var});").unwrap()
                }
                Encoding::Fragment { .. } => writeln!(
                    out,
                    "                __wire.extend_from_slice(&{var}.__oddlang_encode()?);"
                )
                .unwrap(),
                Encoding::Literal { value, .. } => {
                    writeln!(out, "                __wire.push({value});").unwrap()
                }
                Encoding::Enum { id, .. } => {
                    let External::Enum { tags } = &graph.types[id] else {
                        unreachable!()
                    };
                    let native = &binding.types[id];
                    writeln!(out, "                __wire.push(match {var} {{").unwrap();
                    for (id, tag) in tags {
                        writeln!(
                            out,
                            "                    {}::{} => {tag},",
                            native.path, native.variants[id]
                        )
                        .unwrap();
                    }
                    writeln!(out, "                }});").unwrap();
                }
            }
        }
        writeln!(out, "                {}u8\n            }}", case.tag).unwrap();
    }
    let limit = fault(binding, &graph.errors.limit);
    writeln!(out,"        }};\n        if __wire.len() > {maximum} {{ return ::core::result::Result::Err({limit}); }}\n        let mut __out = ::std::vec::Vec::with_capacity(5 + __wire.len());\n        __out.push(__tag);\n        __out.extend_from_slice(&(__wire.len() as u32).to_be_bytes());\n        __out.extend_from_slice(&__wire);\n        ::core::result::Result::Ok(__out)\n    }}").unwrap();
    let header = fault(binding, &graph.errors.header);
    let limit = fault(binding, &graph.errors.limit);
    let length = fault(binding, &graph.errors.length);
    let truncated = fault(binding, &graph.errors.truncated);
    writeln!(out,"    pub fn decode(input: &[u8]) -> ::core::result::Result<Self,{err}> {{\n        if input.len() < 5 {{ return ::core::result::Result::Err({header}); }}\n        let __length = u32::from_be_bytes(input[1..5].try_into().expect(\"header width\")) as usize;\n        if __length > {maximum} {{ return ::core::result::Result::Err({limit}); }}\n        if input.len() != __length + 5 {{ return ::core::result::Result::Err({length}); }}\n        let mut __rest = &input[5..];\n        fn __take<'a>(input: &mut &'a [u8], size: usize) -> ::core::result::Result<&'a [u8],{err}> {{\n           let ::core::option::Option::Some((head,tail)) = input.split_at_checked(size) else {{ return ::core::result::Result::Err({truncated}); }};\n           *input=tail;\n           ::core::result::Result::Ok(head)\n        }}\n        let __value = match input[0] {{").unwrap();
    for (id, case) in &graph.cases {
        writeln!(out, "            {} => {{", case.tag).unwrap();
        for (i, slot) in case.slots.iter().enumerate() {
            let var = format!("__field_{i}");
            match &slot.encoding {
                Encoding::Unsigned{bits,maximum,error}=>{
                    writeln!(out,"                let {var} = u{bits}::from_be_bytes(__take(&mut __rest,{})?.try_into().expect(\"integer width\"));",bits/8).unwrap();
                    if *maximum<u64::MAX>>(64-bits) {
                        writeln!(out,"                if {var} > {maximum} {{ return ::core::result::Result::Err({}); }}",fault(binding,error)).unwrap();
                    }
                },
                Encoding::Fixed{size}=>writeln!(out,"                let {var} = __take(&mut __rest,{size})?.try_into().expect(\"array width\");").unwrap(),
                Encoding::Fragment{id}=>writeln!(out,"                let {var} = {}::__oddlang_read_prefix(&mut __rest)?;\n                {var}.validate()?;",binding.types[id].path).unwrap(),
                Encoding::Literal{value,error}=>writeln!(out,"                if __take(&mut __rest,1)?[0] != {value} {{ return ::core::result::Result::Err({}); }}",fault(binding,error)).unwrap(),
                Encoding::Rest{minimum,maximum,error}=>writeln!(out,
                    "                if !({minimum}..={maximum}).contains(&__rest.len()) {{ return ::core::result::Result::Err({}); }}\n                let {var} = ::core::mem::take(&mut __rest).to_vec();",
                    fault(binding,error)).unwrap(),
                Encoding::Enum{id,error}=>{
                    let External::Enum{tags}=&graph.types[id] else {unreachable!()};
                    let native=&binding.types[id];
                    writeln!(out,"                let {var} = match __take(&mut __rest,1)?[0] {{").unwrap();
                    for (id,tag) in tags {
                        writeln!(out,"                    {tag} => {}::{},",native.path,native.variants[id]).unwrap();
                    }
                    if tags.len()<256 {
                        writeln!(out,"                    _ => return ::core::result::Result::Err({}),",fault(binding,error)).unwrap();
                    }
                    writeln!(out,"                }};").unwrap();
                },
            }
        }
        for guard in &case.decode_guards {
            writeln!(
                out,
                "                if !({}) {{ return ::core::result::Result::Err({}); }}",
                guard_text(case, &guard.require, false),
                fault(binding, &guard.error)
            )
            .unwrap();
        }
        writeln!(
            out,
            "                {}\n            }}",
            pattern(case, binding, &binding.cases[id], |i| format!(
                "__field_{i}"
            ))
        )
        .unwrap();
    }
    if graph.cases.len() < 256 {
        writeln!(
            out,
            "            _ => return ::core::result::Result::Err({}),",
            fault(binding, &graph.errors.tag)
        )
        .unwrap();
    }
    writeln!(out,"        }};\n        if !__rest.is_empty() {{ return ::core::result::Result::Err({}); }}\n        ::core::result::Result::Ok(__value)\n    }}\n}}",
        fault(binding,&graph.errors.trailing)).unwrap();
    decoder(&mut out, graph, binding);
    out
}

fn decoder(out: &mut String, graph: &Graph, binding: &Binding) {
    let decoder = &binding.decoder;
    let frame = &binding.name;
    let error = &binding.error_type;
    let failed = fault(binding, &graph.errors.decoder_failed);
    let truncated = fault(binding, &graph.errors.truncated);
    let limit = fault(binding, &graph.errors.limit);
    let maximum = graph.maximum;
    write!(out,r#"
#[derive(Default)]
pub struct {decoder} {{
    pending: ::std::vec::Vec<u8>,
    expected: ::core::option::Option<usize>,
    failed: bool,
}}
impl {decoder} {{
    pub fn feed(&mut self, input: &[u8]) -> ::core::result::Result<(usize, ::core::option::Option<{frame}>),{error}> {{
        if self.failed {{
            return ::core::result::Result::Err({failed});
        }}
        let result = self.__accept(input);
        if result.is_err() {{
            self.failed = true;
            self.pending.clear();
            self.expected = ::core::option::Option::None;
        }}
        result
    }}

    fn __accept(&mut self, input: &[u8]) -> ::core::result::Result<(usize, ::core::option::Option<{frame}>),{error}> {{
        let header_count = 5usize.saturating_sub(self.pending.len()).min(input.len());
        self.pending.extend_from_slice(&input[..header_count]);
        if self.pending.len() < 5 {{
            return ::core::result::Result::Ok((header_count, ::core::option::Option::None));
        }}
        let target = match self.expected {{
            ::core::option::Option::Some(size) => size,
            ::core::option::Option::None => {{
                let length = u32::from_be_bytes(self.pending[1..5].try_into().expect("header width")) as usize;
                if length > {maximum} {{
                    return ::core::result::Result::Err({limit});
                }}
                let size = length + 5;
                self.expected = ::core::option::Option::Some(size);
                size
            }}
        }};
        let body_count = (target - self.pending.len()).min(input.len() - header_count);
        let count = header_count + body_count;
        self.pending.extend_from_slice(&input[header_count..count]);
        if self.pending.len() < target {{
            return ::core::result::Result::Ok((count, ::core::option::Option::None));
        }}
        let value = {frame}::decode(&self.pending)?;
        self.pending.clear();
        self.expected = ::core::option::Option::None;
        ::core::result::Result::Ok((count, ::core::option::Option::Some(value)))
    }}

    pub fn finish(&self) -> ::core::result::Result<(),{error}> {{
        if self.failed || !self.pending.is_empty() {{
            ::core::result::Result::Err({truncated})
        }} else {{
            ::core::result::Result::Ok(())
        }}
    }}
}}
"#).unwrap();
}
