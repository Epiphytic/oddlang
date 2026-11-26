use super::model::*;
use std::{collections::BTreeMap, fmt::Write as _};

const DECIMAL: &::core::primitive::str = r#"
fn __decimal(text: &::core::primitive::str, min: ::core::primitive::u64, max: ::core::primitive::u64) -> __Option<::core::primitive::u64> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || (bytes[0] == b'0' && bytes.len() > 1) {
        return __None;
    }
    let value = bytes.iter().try_fold(0_u64, |total, &byte| {
        if !byte.is_ascii_digit() {
            return __None;
        }
        total.checked_mul(10)?.checked_add(::core::primitive::u64::from(byte - b'0'))
    })?;
    (min..=max).contains(&value).then_some(value)
}
"#;

const HEX: &::core::primitive::str = r#"
fn __nibble(byte: ::core::primitive::u8) -> __Option<::core::primitive::u8> {
    match byte {
        b'0'..=b'9' => __Some(byte - b'0'),
        b'a'..=b'f' => __Some(byte - b'a' + 10),
        _ => __None,
    }
}

"#;

const DECODE: &str = r#"
fn __decoded<const N: ::core::primitive::usize>(token: &::core::primitive::str) -> __Option<[::core::primitive::u8; N]> {
    let mut bytes = [0; N];
    for (out, pair) in bytes.iter_mut().zip(token.as_bytes().chunks_exact(2)) {
        *out = (__nibble(pair[0])? << 4) | __nibble(pair[1])?;
    }
    __Some(bytes)
}
"#;

struct Renderer<'a> {
    source: String,
    grammars: BTreeMap<&'a str, usize>,
    captures: BTreeMap<&'a str, usize>,
    locals: usize,
    uses_decimal: bool,
    uses_hex: bool,
    uses_decode: bool,
}

impl<'a> Renderer<'a> {
    fn bind(&mut self, id: Option<&'a str>, rhs: String) {
        if let Some(id) = id {
            let index = self.captures.len();
            self.captures.insert(id, index);
            writeln!(self.source, "    let __capture_{index} = {rhs};").unwrap();
        } else {
            writeln!(self.source, "    {rhs};").unwrap();
        }
    }

    fn guard(&mut self, condition: String) {
        writeln!(self.source, "    if !({condition}) {{ return __None; }}").unwrap();
    }

    fn token(&mut self, token: &'a Token, input: &str) {
        match token {
            Token::Literal { text, capture } => {
                self.guard(format!("{input} == {text:?}"));
                if capture.is_some() {
                    self.bind(capture.as_deref(), format!("{text:?}"));
                }
            }
            Token::Text { capture, choices, grammar } => {
                self.guard(format!("!{input}.is_empty()"));
                if !choices.is_empty() {
                    self.guard(format!("{choices:?}.contains(&{input})"));
                }
                if let Some(grammar) = grammar {
                    let index = self.grammars[grammar.as_str()];
                    self.guard(format!("__grammar_{index}::accepts({input})"));
                }
                if capture.is_some() {
                    self.bind(capture.as_deref(), input.to_owned());
                }
            }
            Token::Decimal { capture, bits, minimum, maximum } => {
                self.uses_decimal = true;
                let maximum = maximum.unwrap_or_else(|| unsigned_maximum(*bits).expect("validated width"));
                let parsed = format!("__decimal({input}, {minimum}, {maximum})?");
                let rhs = if capture.is_some() { format!("{parsed} as ::core::primitive::u{bits}") } else { parsed };
                self.bind(capture.as_deref(), rhs);
            }
            Token::Radix16 { capture, digits, decode } => {
                self.uses_hex = true;
                self.uses_decode |= *decode;
                self.guard(format!("{input}.len() == {digits}"));
                let rhs = if *decode {
                    format!("__decoded::<{}>({input})?", digits / 2)
                } else {
                    self.guard(format!("{input}.bytes().all(|byte| __nibble(byte).is_some())"));
                    input.to_owned()
                };
                self.bind(Some(capture), rhs);
            }
            Token::Boolean { capture, false_token, true_token } => self.bind(
                Some(capture),
                format!("match {input} {{ {false_token:?} => false, {true_token:?} => true, _ => return __None }}"),
            ),
            Token::IntegerToken { capture, text, bits, value } => {
                self.guard(format!("{input} == {text:?}"));
                self.bind(Some(capture), format!("{value}_u{bits}"));
            }
            Token::Split { capture, delimiter, left, right } => {
                if capture.is_some() {
                    self.bind(capture.as_deref(), input.to_owned());
                }
                let slot = self.locals;
                self.locals += 1;
                let a = format!("__left_{slot}");
                let b = format!("__right_{slot}");
                writeln!(self.source, "    let ({a}, {b}) = {input}.split_once({delimiter:?})?;").unwrap();
                self.token(left, &a);
                self.token(right, &b);
            }
        }
    }
}

pub(super) fn render(graph: &Graph, overlay: &Binding) -> Result<String> {
    let (captures, grammars) = graph.validate(overlay)?;
    let mut render = Renderer {
        source: "use ::core::option::Option::{None as __None, Some as __Some};\nuse ::core::option::Option as __Option;\n".into(),
        grammars,
        captures: BTreeMap::new(),
        locals: 0,
        uses_decimal: false,
        uses_hex: false,
        uses_decode: false,
    };
    for (index, grammar) in graph.text_grammars.iter().enumerate() {
        writeln!(
            render.source,
            "mod __grammar_{index} {{\n{}\n}}",
            grammar.render()?
        )
        .unwrap();
    }
    if let (Binding::Records(binding), Some(result)) = (overlay, &graph.record_result) {
        result.declarations(binding, &mut render.source);
    } else if let Binding::Captures(binding) = overlay {
        let (generic, lifetime) = if captures.values().any(|t| *t == Kind::Text) {
            ("<'input>", "<'_>")
        } else {
            ("", "")
        };
        writeln!(render.source, "pub struct {}{generic} {{", binding.record).unwrap();
        for (capture, ty) in captures {
            writeln!(
                render.source,
                "    pub {}: {},",
                binding.capture_names[capture],
                ty.rust_type()
            )
            .unwrap();
        }
        writeln!(
            render.source,
            "}}\n\npub fn {}(input: &[::core::primitive::u8]) -> __Option<{}{lifetime}> {{",
            binding.reader, binding.record
        )
        .unwrap();
    }
    render.guard(format!("input.len() <= {}", graph.max_bytes));
    render.guard("input.is_ascii()".into());
    if !graph.forbidden_bytes.is_empty() {
        render.guard(format!(
            "!input.iter().any(|byte| {:?}.contains(byte))",
            graph.forbidden_bytes
        ));
    }
    render.source.push_str(
        "    let text = ::core::str::from_utf8(input).ok()?.strip_suffix('\\n')?;\n    let mut rows = text.split('\\n');\n",
    );
    let mut constraints = graph.constraints.iter().peekable();
    for (index, row) in graph.rows.iter().enumerate() {
        render
            .source
            .push_str("    let mut tokens = rows.next()?.split(' ');\n");
        for token in row {
            render
                .source
                .push_str("    let token = tokens.next().filter(|text| !text.is_empty())?;\n");
            render.token(token, "token");
        }
        render.guard("tokens.next().is_none()".into());
        while let Some(constraint) = constraints.next_if(|constraint| constraint.row() == index + 1)
        {
            render.guard(constraint.rust_condition(&render.captures));
        }
    }
    render.guard("rows.next().is_none()".into());
    if let (Binding::Records(binding), Some(result)) = (overlay, &graph.record_result) {
        writeln!(
            render.source,
            "    __Some({})",
            result.root(binding, &render.captures)
        )
        .unwrap();
    } else if let Binding::Captures(binding) = overlay {
        writeln!(render.source, "    __Some({} {{", binding.record).unwrap();
        for (capture, index) in &render.captures {
            writeln!(
                render.source,
                "        {}: __capture_{index},",
                binding.capture_names[*capture]
            )
            .unwrap();
        }
        render.source.push_str("    })\n");
    }
    render.source.push_str("}\n");
    if render.uses_decimal {
        render.source.push_str(DECIMAL);
    }
    if render.uses_hex {
        render.source.push_str(HEX);
    }
    if render.uses_decode {
        render.source.push_str(DECODE);
    }
    Ok(render.source)
}
