use std::collections::BTreeSet;
use url::{Host, Url};

type Error = &'static str;

struct InputRule {
    unicode_whitespace: bool,
    unicode_control: bool,
    characters: &'static str,
    error: Error,
}

enum Rule {
    AllowedSchemes {
        schemes: &'static [&'static str],
        error: Error,
    },
    UrlComponents {
        forbid: &'static [Component],
        allowed_paths: Option<&'static [&'static str]>,
        error: Error,
    },
    HostPolicy {
        dns: DnsPolicy,
        ip: IpPolicy,
        error: Error,
    },
    LoopbackOptIn {
        error: Error,
    },
}

enum Component {
    Username,
    Password,
    Query,
    Fragment,
    Port,
}

impl Component {
    fn is_present(&self, value: &Url) -> bool {
        match self {
            Self::Username => !value.username().is_empty(),
            Self::Password => value.password().is_some(),
            Self::Query => value.query().is_some(),
            Self::Fragment => value.fragment().is_some(),
            Self::Port => value.port().is_some(),
        }
    }
}

#[derive(Clone, Copy)]
enum DnsSyntax {
    AsciiLabels,
    Any,
    Deny,
}

#[derive(Clone, Copy)]
enum TrailingDot {
    Preserve,
    StripOne,
    Reject,
}

struct DnsPolicy {
    syntax: DnsSyntax,
    trailing_dot: TrailingDot,
    loopback_suffixes: &'static [&'static str],
    rewrite_error: Error,
}

impl DnsPolicy {
    fn classify(&self, host: &str, error: Error) -> Result<(String, bool), Error> {
        let label_name = host.strip_suffix('.').unwrap_or(host);
        if label_name.is_empty()
            || matches!(self.syntax, DnsSyntax::Deny)
            || (matches!(self.trailing_dot, TrailingDot::Reject) && label_name != host)
            || (matches!(self.syntax, DnsSyntax::AsciiLabels) && !valid_dns_labels(label_name))
        {
            return Err(error);
        }
        let name = label_name.to_ascii_lowercase();
        let loopback = self.loopback_suffixes.iter().any(|suffix| {
            name == *suffix
                || name
                    .strip_suffix(suffix)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        });
        Ok((label_name.to_owned(), loopback))
    }
}

struct Prefix {
    network: u128,
    mask: u128,
}

impl Prefix {
    fn contains(&self, address: u128) -> bool {
        address & self.mask == self.network
    }
}

struct FamilyPolicy {
    allow: &'static [Prefix],
    deny: &'static [Prefix],
    default_allow: bool,
}

impl FamilyPolicy {
    fn classify(&self, address: u128, loopback: &[Prefix], error: Error) -> Result<bool, Error> {
        if loopback.iter().any(|prefix| prefix.contains(address)) {
            return Ok(true);
        }
        if self.deny.iter().any(|prefix| prefix.contains(address)) {
            return Err(error);
        }
        if self.default_allow || self.allow.iter().any(|prefix| prefix.contains(address)) {
            return Ok(false);
        }
        Err(error)
    }
}

struct IpPolicy {
    loopback_v4: &'static [Prefix],
    loopback_v6: &'static [Prefix],
    ipv4: FamilyPolicy,
    ipv6: FamilyPolicy,
}

fn normalize_entry(entry: &str, allow_loopback: bool) -> Result<String, Error> {
    for rule in INPUT_RULES {
        if entry.chars().any(|ch| {
            (rule.unicode_whitespace && ch.is_whitespace())
                || (rule.unicode_control && ch.is_control())
                || rule.characters.contains(ch)
        }) {
            return Err(rule.error);
        }
    }
    let mut value = Url::parse(entry).map_err(|_| PARSE_ERROR)?;
    let mut loopback = false;
    for rule in RULES {
        match rule {
            Rule::AllowedSchemes { schemes, error } => {
                if !schemes.contains(&value.scheme()) {
                    return Err(error);
                }
            }
            Rule::UrlComponents {
                forbid,
                allowed_paths,
                error,
            } => {
                if forbid.iter().any(|component| component.is_present(&value))
                    || allowed_paths.is_some_and(|paths| !paths.contains(&value.path()))
                {
                    return Err(error);
                }
            }
            Rule::HostPolicy { dns, ip, error } => {
                loopback = match value.host().ok_or(*error)? {
                    Host::Domain(host) => {
                        let (canonical, is_loopback) = dns.classify(host, error)?;
                        if matches!(dns.trailing_dot, TrailingDot::StripOne) {
                            value
                                .set_host(Some(&canonical))
                                .map_err(|_| dns.rewrite_error)?;
                        }
                        is_loopback
                    }
                    Host::Ipv4(address) => {
                        ip.ipv4
                            .classify(u32::from(address) as u128, ip.loopback_v4, error)?
                    }
                    Host::Ipv6(address) => {
                        ip.ipv6
                            .classify(u128::from(address), ip.loopback_v6, error)?
                    }
                };
            }
            Rule::LoopbackOptIn { error } => {
                if loopback && !allow_loopback {
                    return Err(error);
                }
            }
        }
    }
    Ok(value.to_string())
}

pub(super) fn normalize(entries: &[String], allow_loopback: bool) -> Result<Vec<String>, Error> {
    if let Some(error) = EMPTY_ERROR.filter(|_| entries.is_empty()) {
        return Err(error);
    }
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in entries {
        let canonical = normalize_entry(entry, allow_loopback)?;
        if !DEDUPLICATE || seen.insert(canonical.clone()) {
            output.push(canonical);
        }
    }
    Ok(output)
}
