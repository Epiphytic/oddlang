use super::{Framing, Result};
use oxigraph::{
    io::{RdfFormat, RdfParser},
    model::{Literal, NamedOrBlankNode, Term},
};
use std::collections::BTreeMap;
const NS: &str = "urn:oddlang:framing:";
fn iri(id: &str) -> String {
    format!("<{NS}{id}>")
}
impl Framing {
    pub(super) fn graph(&self) -> String {
        let mut lines = Vec::new();
        let mut triple =
            |s: &str, p: &str, o: String| lines.push(format!("{} {} {o} .", iri(s), iri(p)));
        for (s, p, o) in [
            ("codec", "profile", "framing-v1"),
            ("codec", "nativeType", "ProbeEnvelope"),
            ("codec", "errorType", "FrameError"),
            ("codec", "limitsType", "Limits"),
            ("codec", "wireRecord", "Envelope"),
            ("Envelope", "fields", "version-then-payload"),
            ("Envelope", "onUnknown", "reject"),
            ("Envelope", "onDuplicate", "reject"),
            ("version", "wireType", "u32"),
            ("payload", "wireType", "utf8-string"),
            ("payload", "encoding", "rfc4648-standard-padded-canonical"),
            ("encode", "argumentType", "octets"),
            ("decode", "argumentType", "octets"),
            ("encode", "resultType", "octets-or-FrameError"),
            ("decode", "resultType", "octets-or-FrameError"),
            ("RAW_BYTES", "use", "enforced"),
            ("JSON_BYTES", "use", "enforced"),
            ("NDJSON_BYTES", "use", "declared"),
        ] {
            triple(s, p, iri(o));
        }
        for (s, v) in [
            ("RAW_BYTES", self.raw_limit),
            ("JSON_BYTES", self.json_limit),
            ("NDJSON_BYTES", self.ndjson_limit),
            ("version", u64::from(self.version)),
        ] {
            triple(s, "value", Literal::from(v).to_string());
        }
        for (name, msg) in [
            "SizeLimit",
            "InvalidJson",
            "InvalidVersion",
            "InvalidEncoding",
        ]
        .iter()
        .zip(&self.errors)
        {
            triple(name, "display", Literal::from(msg.as_str()).to_string());
        }
        for (op, steps) in [
            ("encode", &self.encode_steps),
            ("decode", &self.decode_steps),
        ] {
            for (index, step) in steps.iter().enumerate() {
                triple(op, &format!("step-{}", index + 1), iri(step));
            }
        }
        lines.sort();
        lines.join("\n") + "\n"
    }
    pub(super) fn from_graph(bytes: &[u8]) -> Result<Self> {
        let mut values = BTreeMap::new();
        for quad in RdfParser::from_format(RdfFormat::NTriples).for_reader(bytes) {
            let q = quad.map_err(|e| format!("invalid framing graph: {e}"))?;
            let NamedOrBlankNode::NamedNode(s) = q.subject else {
                return Err("framing graph requires named subjects".into());
            };
            let s = s
                .as_str()
                .strip_prefix(NS)
                .ok_or("unexpected subject namespace")?;
            let p = q
                .predicate
                .as_str()
                .strip_prefix(NS)
                .ok_or("unexpected predicate namespace")?;
            let o = match q.object {
                Term::NamedNode(n) => n
                    .as_str()
                    .strip_prefix(NS)
                    .ok_or("unexpected object namespace")?
                    .to_owned(),
                Term::Literal(l) => l.value().to_owned(),
                _ => return Err("unsupported framing object".into()),
            };
            if values.insert((s.to_owned(), p.to_owned()), o).is_some() {
                return Err("duplicate framing predicate".into());
            }
        }
        let get = |s: &str, p: &str| {
            values
                .get(&(s.to_owned(), p.to_owned()))
                .cloned()
                .ok_or_else(|| format!("missing framing {s} {p}"))
        };
        let number = |s: &str| {
            get(s, "value")?
                .parse::<u64>()
                .map_err(|_| format!("invalid {s} value"))
        };
        let steps = |s: &str, n: usize| {
            (1..=n)
                .map(|i| get(s, &format!("step-{i}")))
                .collect::<Result<Vec<_>>>()
        };
        let f = Self {
            raw_limit: number("RAW_BYTES")?,
            json_limit: number("JSON_BYTES")?,
            ndjson_limit: number("NDJSON_BYTES")?,
            version: number("version")?
                .try_into()
                .map_err(|_| "version does not fit u32")?,
            errors: [
                get("SizeLimit", "display")?,
                get("InvalidJson", "display")?,
                get("InvalidVersion", "display")?,
                get("InvalidEncoding", "display")?,
            ],
            encode_steps: steps("encode", 4)?,
            decode_steps: steps("decode", 8)?,
        };
        f.validate()?;
        if f.graph().as_bytes() != bytes {
            return Err("unrecognized or noncanonical framing graph".into());
        }
        Ok(f)
    }
}
