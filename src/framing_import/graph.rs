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
    /// A single traversal defines both canonical triples and their source witnesses.
    pub(super) fn facts(&self) -> BTreeMap<String, Vec<String>> {
        let mut facts = BTreeMap::new();
        let mut triple = |s: &str, p: &str, o: String, sites: Vec<String>| {
            facts.insert(format!("{} {} {o} .", iri(s), iri(p)), sites);
        };
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
            let sites: &[&str] = match (s, p) {
                ("codec", "profile") => &["ProbeEnvelope.impl"],
                ("codec", "nativeType") => &["ProbeEnvelope"],
                ("codec", "errorType") => &["FrameError"],
                ("codec", "limitsType") => &["Limits"],
                ("codec", "wireRecord") | ("Envelope", _) | (_, "wireType") => &["Envelope"],
                ("payload", "encoding") => &[
                    "base64",
                    "encode.json-encode-base64",
                    "decode.base64-decode",
                    "decode.canonical",
                ],
                ("encode", _) => &["encode.signature"],
                ("decode", _) => &["decode.signature"],
                ("RAW_BYTES", "use") => &[
                    "Limits.RAW_BYTES",
                    "encode.raw-limit",
                    "decode.raw-limit",
                    "decode.encoded-bound",
                ],
                ("JSON_BYTES", "use") => &[
                    "Limits.JSON_BYTES",
                    "encode.json-limit",
                    "decode.json-limit",
                ],
                ("NDJSON_BYTES", "use") => &["Limits.NDJSON_BYTES"],
                _ => unreachable!("static framing vocabulary"),
            };
            triple(s, p, iri(o), sites.iter().map(|s| s.to_string()).collect());
        }
        for (s, v) in [
            ("RAW_BYTES", self.raw_limit),
            ("JSON_BYTES", self.json_limit),
            ("NDJSON_BYTES", self.ndjson_limit),
            ("version", u64::from(self.version)),
        ] {
            let sites = if s == "version" {
                vec!["encode.json-encode-base64".into(), "decode.version".into()]
            } else {
                vec![format!("Limits.{s}")]
            };
            triple(s, "value", Literal::from(v).to_string(), sites);
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
            triple(
                name,
                "display",
                Literal::from(msg.as_str()).to_string(),
                vec![format!("FrameError.{name}")],
            );
        }
        for (op, steps) in [
            ("encode", &self.encode_steps),
            ("decode", &self.decode_steps),
        ] {
            for (index, step) in steps.iter().enumerate() {
                triple(
                    op,
                    &format!("step-{}", index + 1),
                    iri(step),
                    vec![format!("{op}.{step}")],
                );
            }
        }
        facts
    }
    pub(super) fn graph(&self) -> String {
        self.facts().into_keys().collect::<Vec<_>>().join("\n") + "\n"
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
