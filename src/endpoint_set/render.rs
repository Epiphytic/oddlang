use super::schema::{Empty, Family, FamilyPolicy, Graph, Prefix, Rule, RustOverlay};
use std::fmt::Write;

pub(super) fn rust(graph: &Graph, overlay: Option<&RustOverlay>) -> String {
    let mut implementation = include_str!("native.rs").to_owned();
    implementation.push_str(include_str!("dns.rs"));
    let empty = match &graph.empty {
        Empty::Allow {} => "None".to_owned(),
        Empty::Reject { error } => format!("Some({error:?})"),
    };
    writeln!(
        implementation,
        "const EMPTY_ERROR: Option<Error> = {empty};\nconst DEDUPLICATE: bool = {};",
        graph.deduplicate,
    )
    .unwrap();
    implementation.push_str("const INPUT_RULES: &[InputRule] = &[\n");
    for rule in &graph.rules {
        match rule {
            Rule::ForbiddenInput {
                unicode_whitespace,
                unicode_control,
                characters,
                error,
            } => {
                writeln!(
                    implementation,
                    "    InputRule {{ unicode_whitespace: {unicode_whitespace}, unicode_control: {unicode_control}, characters: {characters:?}, error: {error:?} }},",
                )
                .unwrap();
            }
            Rule::Parse { error } => {
                writeln!(implementation, "];\nconst PARSE_ERROR: Error = {error:?};").unwrap();
                break;
            }
            _ => unreachable!("validated parse order"),
        }
    }
    implementation.push_str("const RULES: &[Rule] = &[\n");
    for rule in &graph.rules {
        if let Some(rendered) = render_rule(rule) {
            writeln!(implementation, "    {rendered},").unwrap();
        }
    }
    implementation.push_str("];\n");

    let mut out = "#[allow(dead_code)]\nmod endpoint_set_impl {\n".to_owned();
    for line in implementation.lines() {
        if !line.is_empty() {
            out.push_str("    ");
            out.push_str(line);
        }
        out.push('\n');
    }
    out.push_str("}\n\n");
    match overlay {
        Some(overlay) => write_binding(&mut out, overlay),
        None => out.push_str(
            "pub fn normalize(entries: &[String], allow_loopback: bool) -> Result<Vec<String>, &'static str> {\n    endpoint_set_impl::normalize(entries, allow_loopback)\n}\n",
        ),
    }
    out
}

fn render_rule(rule: &Rule) -> Option<String> {
    Some(match rule {
        Rule::ForbiddenInput { .. } | Rule::Parse { .. } => return None,
        Rule::AllowedSchemes { schemes, error } => {
            format!("Rule::AllowedSchemes {{ schemes: &{schemes:?}, error: {error:?} }}")
        }
        Rule::UrlComponents {
            forbid,
            allowed_paths,
            error,
        } => {
            let components = forbid
                .iter()
                .map(|component| format!("Component::{component:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            let paths = match allowed_paths {
                Some(paths) => format!("Some(&{paths:?})"),
                None => "None".to_owned(),
            };
            format!(
                "Rule::UrlComponents {{ forbid: &[{components}], allowed_paths: {paths}, error: {error:?} }}"
            )
        }
        Rule::HostPolicy { dns, ip, error } => {
            let dns = format!(
                "DnsPolicy {{ syntax: DnsSyntax::{:?}, trailing_dot: TrailingDot::{:?}, loopback_suffixes: &{:?}, rewrite_error: {:?} }}",
                dns.syntax, dns.trailing_dot, dns.loopback_suffixes, dns.rewrite_error
            );
            let loopback_v4 = prefixes(&ip.loopback, Family::V4);
            let loopback_v6 = prefixes(&ip.loopback, Family::V6);
            let ipv4 = family_policy(&ip.ipv4, Family::V4);
            let ipv6 = family_policy(&ip.ipv6, Family::V6);
            format!(
                "Rule::HostPolicy {{ dns: {dns}, ip: IpPolicy {{ loopback_v4: {loopback_v4}, loopback_v6: {loopback_v6}, ipv4: {ipv4}, ipv6: {ipv6} }}, error: {error:?} }}"
            )
        }
        Rule::LoopbackOptIn { error } => {
            format!("Rule::LoopbackOptIn {{ error: {error:?} }}")
        }
    })
}

fn prefixes(list: &[String], family: Family) -> String {
    let entries = list
        .iter()
        .map(|entry| Prefix::parse(entry).expect("validated IP prefix"))
        .filter(|entry| entry.family == family)
        .map(|entry| {
            format!(
                "Prefix {{ network: {:#x}, mask: {:#x} }}",
                entry.network, entry.mask
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{entries}]")
}

fn family_policy(policy: &FamilyPolicy, family: Family) -> String {
    let allow = prefixes(&policy.allow, family);
    let deny = prefixes(&policy.deny, family);
    format!(
        "FamilyPolicy {{ allow: {allow}, deny: {deny}, default_allow: {} }}",
        policy.default_allow,
    )
}

fn write_binding(out: &mut String, overlay: &RustOverlay) {
    if !overlay.derives.is_empty() {
        let derives = overlay
            .derives
            .iter()
            .map(|derive| format!("{derive:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "#[derive({derives})]").unwrap();
    }
    let name = &overlay.type_name;
    let constructor = &overlay.constructor;
    let getter = &overlay.getter;
    let error_type = &overlay.error_type;
    writeln!(
        out,
        "pub struct {name}(Vec<String>);\n\nimpl {name} {{\n    pub fn {constructor}(entries: &[String], allow_loopback: bool) -> Result<Self, {error_type}> {{\n        endpoint_set_impl::normalize(entries, allow_loopback).map(Self).map_err(|error| match error {{",
    )
    .unwrap();
    for (error, variant) in &overlay.error_variants {
        writeln!(out, "            {error:?} => {error_type}::{variant},").unwrap();
    }
    writeln!(
        out,
        "            _ => unreachable!(\"validated endpoint-set error mapping\"),\n        }})\n    }}\n\n    pub fn {getter}(&self) -> &[String] {{\n        &self.0\n    }}\n}}",
    )
    .unwrap();
}
