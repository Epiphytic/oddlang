use oxigraph::model::NamedNode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    net::IpAddr,
    path::Path,
};

pub(super) const VOCABULARY: &str = "oddlang-endpoint-set-v1";
pub(super) const RUST_VOCABULARY: &str = "oddlang-endpoint-set-rust-v1";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Graph {
    pub vocabulary: String,
    pub id: String,
    pub parser: Parser,
    pub provider_hint: Option<ProviderHint>,
    pub errors: Vec<String>,
    pub empty: Empty,
    pub deduplicate: bool,
    pub rules: Vec<Rule>,
}

#[derive(Deserialize, Serialize)]
pub(super) enum Parser {
    #[serde(rename = "whatwg-url")]
    WhatwgUrl,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProviderHint {
    cargo_package: String,
    version: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Empty {
    Allow,
    Reject { error: String },
}

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Rule {
    ForbiddenInput {
        unicode_whitespace: bool,
        unicode_control: bool,
        characters: String,
        error: String,
    },
    Parse {
        error: String,
    },
    AllowedSchemes {
        schemes: Vec<String>,
        error: String,
    },
    UrlComponents {
        forbid: Vec<Component>,
        allowed_paths: Option<Vec<String>>,
        error: String,
    },
    HostPolicy {
        dns: DnsPolicy,
        ip: IpPolicy,
        error: String,
    },
    LoopbackOptIn {
        error: String,
    },
}

#[derive(Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Component {
    Username,
    Password,
    Query,
    Fragment,
    Port,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DnsPolicy {
    pub syntax: DnsSyntax,
    pub trailing_dot: TrailingDot,
    pub loopback_suffixes: Vec<String>,
    pub rewrite_error: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum DnsSyntax {
    AsciiLabels,
    Any,
    Deny,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum TrailingDot {
    Preserve,
    StripOne,
    Reject,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IpPolicy {
    pub loopback: Vec<String>,
    pub ipv4: FamilyPolicy,
    pub ipv6: FamilyPolicy,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FamilyPolicy {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub default_allow: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    V4,
    V6,
}

pub(super) struct Prefix {
    pub family: Family,
    pub network: u128,
    pub mask: u128,
}

impl Prefix {
    pub fn parse(input: &str) -> Result<Self, String> {
        let invalid = || format!("invalid IP prefix {input:?}");
        let (address, length) = input.split_once('/').ok_or_else(invalid)?;
        let ip = address.parse::<IpAddr>().map_err(|_| invalid())?;
        let bits = length.parse::<u8>().map_err(|_| invalid())?;
        let (family, width, network) = match ip {
            IpAddr::V4(ip) => (Family::V4, 32, u32::from(ip) as u128),
            IpAddr::V6(ip) => (Family::V6, 128, u128::from(ip)),
        };
        if bits > width || address != ip.to_string() || length != bits.to_string() {
            return Err(invalid());
        }
        let mask = if bits == 0 {
            0
        } else {
            let host_bits = width - bits;
            ((u128::MAX >> (128 - width)) >> host_bits) << host_bits
        };
        if network & !mask != 0 {
            return Err(format!("nonzero host bits in IP prefix {input:?}"));
        }
        Ok(Self {
            family,
            network,
            mask,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RustOverlay {
    pub vocabulary: String,
    pub type_name: String,
    pub constructor: String,
    pub getter: String,
    pub error_type: String,
    pub error_variants: BTreeMap<String, String>,
    pub derives: Vec<Derive>,
}

#[derive(Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) enum Derive {
    Clone,
    Debug,
    PartialEq,
    Eq,
}

pub(super) fn load_graph(path: &Path) -> Result<Graph, String> {
    let mut graph: Graph = read(path, "endpoint-set graph")?;
    graph.validate()?;
    graph.errors.sort_unstable();
    for rule in &mut graph.rules {
        match rule {
            Rule::ForbiddenInput { characters, .. } => {
                *characters = characters
                    .chars()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
            }
            Rule::AllowedSchemes { schemes, .. } => canonical_set(schemes),
            Rule::UrlComponents {
                forbid,
                allowed_paths,
                ..
            } => {
                canonical_set(forbid);
                if let Some(paths) = allowed_paths {
                    canonical_set(paths);
                }
            }
            Rule::HostPolicy { dns, ip, .. } => {
                dns.loopback_suffixes
                    .iter_mut()
                    .for_each(|name| name.make_ascii_lowercase());
                canonical_set(&mut dns.loopback_suffixes);
                canonical_set(&mut ip.loopback);
                for family in [&mut ip.ipv4, &mut ip.ipv6] {
                    canonical_set(&mut family.allow);
                    canonical_set(&mut family.deny);
                }
            }
            Rule::Parse { .. } | Rule::LoopbackOptIn { .. } => {}
        }
    }
    Ok(graph)
}

pub(super) fn load_overlay(path: &Path) -> Result<RustOverlay, String> {
    let mut overlay: RustOverlay = read(path, "endpoint-set Rust overlay")?;
    overlay.validate()?;
    overlay.derives.sort_unstable();
    Ok(overlay)
}

fn canonical_set<T: Ord>(values: &mut Vec<T>) {
    values.sort_unstable();
    values.dedup();
}

pub(super) fn sha256<T: Serialize>(value: &T) -> Result<String, String> {
    let bytes =
        serde_json::to_vec(value).map_err(|err| format!("encode endpoint-set pin: {err}"))?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn read<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|err| format!("read {kind}: {err}"))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("invalid {kind}: {err}"))
}

impl Graph {
    fn validate(&self) -> Result<(), String> {
        if self.vocabulary != VOCABULARY {
            return Err("unsupported endpoint-set vocabulary".into());
        }
        identifier(&self.id)?;
        if let Some(hint) = &self.provider_hint
            && (hint.cargo_package.is_empty() || hint.version.is_empty())
        {
            return Err("empty endpoint provider hint".into());
        }
        let errors: BTreeSet<_> = self.errors.iter().map(String::as_str).collect();
        if errors.is_empty() || errors.len() != self.errors.len() {
            return Err("endpoint-set errors must be nonempty and distinct".into());
        }
        for error in &errors {
            identifier(error)?;
        }
        let check_error = |error: &str| {
            errors
                .contains(error)
                .then_some(())
                .ok_or_else(|| format!("undeclared error {error:?}"))
        };
        if let Empty::Reject { error } = &self.empty {
            check_error(error)?;
        }
        let mut parsed = false;
        let mut classified = false;
        let mut opted_in = false;
        for rule in &self.rules {
            let error = match rule {
                Rule::ForbiddenInput { error, .. } => {
                    if parsed {
                        return Err("input rule must precede parse".into());
                    }
                    error
                }
                Rule::Parse { error } => {
                    if parsed {
                        return Err("endpoint-set requires exactly one parse rule".into());
                    }
                    parsed = true;
                    error
                }
                _ if !parsed => return Err("URL rule must follow parse".into()),
                Rule::AllowedSchemes { schemes, error } => {
                    if schemes.iter().any(|scheme| !valid_scheme(scheme)) {
                        return Err("invalid canonical scheme".into());
                    }
                    error
                }
                Rule::UrlComponents { error, .. } => error,
                Rule::HostPolicy { dns, ip, error } => {
                    if classified {
                        return Err("endpoint-set accepts one host-policy rule".into());
                    }
                    if dns
                        .loopback_suffixes
                        .iter()
                        .any(|name| !valid_dns_labels(name))
                    {
                        return Err("invalid DNS loopback suffix".into());
                    }
                    check_error(&dns.rewrite_error)?;
                    ip.validate()?;
                    classified = true;
                    error
                }
                Rule::LoopbackOptIn { error } => {
                    if !classified || opted_in {
                        return Err("one loopback-opt-in may follow host-policy".into());
                    }
                    opted_in = true;
                    error
                }
            };
            check_error(error)?;
        }
        if !parsed {
            return Err("endpoint-set requires exactly one parse rule".into());
        }
        Ok(())
    }
}

impl IpPolicy {
    fn validate(&self) -> Result<(), String> {
        for prefix in &self.loopback {
            Prefix::parse(prefix)?;
        }
        for (policy, family) in [(&self.ipv4, Family::V4), (&self.ipv6, Family::V6)] {
            for cidr in policy.allow.iter().chain(&policy.deny) {
                if Prefix::parse(cidr)?.family != family {
                    return Err(format!("wrong address family for IP prefix {cidr:?}"));
                }
            }
        }
        Ok(())
    }
}

impl RustOverlay {
    fn validate(&self) -> Result<(), String> {
        if self.vocabulary != RUST_VOCABULARY {
            return Err("unsupported endpoint-set Rust overlay vocabulary".into());
        }
        for ident in [&self.type_name, &self.constructor, &self.getter] {
            rust_ident(ident)?;
        }
        if canonical_ident(&self.constructor) == canonical_ident(&self.getter) {
            return Err("constructor and getter must have distinct Rust names".into());
        }
        if canonical_ident(&self.type_name) == "endpoint_set_impl" {
            return Err("reserved endpoint-set Rust type name".into());
        }
        let path = syn::parse_str::<syn::Path>(&self.error_type)
            .map_err(|_| "invalid Rust error type path".to_owned())?;
        if path.segments.iter().any(|part| !part.arguments.is_none()) {
            return Err("Rust error type must be a simple path".into());
        }
        let joined = path
            .segments
            .iter()
            .map(|part| part.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        let leading = if path.leading_colon.is_some() {
            "::"
        } else {
            ""
        };
        if self.error_type != format!("{leading}{joined}") {
            return Err("Rust error type must be a canonical path".into());
        }
        for (error, variant) in &self.error_variants {
            identifier(error)?;
            rust_ident(variant)?;
        }
        let derives: BTreeSet<_> = self.derives.iter().collect();
        if derives.len() != self.derives.len()
            || (derives.contains(&Derive::Eq) && !derives.contains(&Derive::PartialEq))
        {
            return Err("invalid checked endpoint type derives".into());
        }
        Ok(())
    }

    pub fn validate_bindings(&self, graph: &Graph) -> Result<(), String> {
        if self.error_variants.keys().ne(graph.errors.iter()) {
            return Err("Rust error mapping must cover exactly the declared errors".into());
        }
        Ok(())
    }
}

fn identifier(value: &str) -> Result<(), String> {
    NamedNode::new(value)
        .map(|_| ())
        .map_err(|_| format!("invalid endpoint identity {value:?}"))
}

fn rust_ident(value: &str) -> Result<(), String> {
    syn::parse_str::<syn::Ident>(value)
        .ok()
        .filter(|ident| ident.to_string() == value)
        .map(|_| ())
        .ok_or_else(|| format!("invalid Rust identifier {value:?}"))
}

fn canonical_ident(value: &str) -> &str {
    value.strip_prefix("r#").unwrap_or(value)
}

fn valid_scheme(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
        })
}

include!("dns.rs");
