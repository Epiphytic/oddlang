use super::{Diagnostic, component, distinct};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Configured {
    pub(super) leaf: String,
    maximum: u32,
    relays: String,
    route: String,
    auto_backend: String,
    default_route: String,
    direct_routes: Vec<String>,
    maximum_relays: u16,
    missing_relays: Diagnostic,
    relay_rules: RelayRules,
    pub(super) fixed: serde_json::Map<String, serde_json::Value>,
    sources: Vec<StateSource>,
    missing_state: Diagnostic,
    malformed: Diagnostic,
    oversized: Diagnostic,
    conflict: Diagnostic,
    pub(super) storage: super::schema::ConfiguredStorage,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RelayRules {
    maximum_bytes: u32,
    rejected_host_suffixes: Vec<String>,
    rejected_characters: String,
    reject_control: bool,
    reject_whitespace: bool,
    reject_credentials: bool,
    reject_query: bool,
    reject_fragment: bool,
    reject_zero_port: bool,
    reject_empty_port: bool,
    reject_padded_port: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StateSource {
    variable: String,
    platform: Option<Platform>,
    require_absolute: bool,
    suffix: Vec<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Platform {
    Linux,
    Macos,
}

impl Configured {
    pub(super) fn render(&self, out: &mut String, provisioning: bool) -> Result<(), String> {
        if !component(&self.leaf)
            || !(1..=16_777_216).contains(&self.maximum)
            || !distinct(
                self.fixed
                    .keys()
                    .map(String::as_str)
                    .chain([self.relays.as_str(), self.route.as_str()]),
            )
            || !distinct(self.direct_routes.iter().map(String::as_str))
            || !self.direct_routes.contains(&self.default_route)
            || !component(&self.auto_backend)
            || self.maximum_relays == 0
            || self.sources.is_empty()
            || self.sources.iter().any(|s| {
                s.variable.is_empty()
                    || !s
                        .variable
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    || !s.suffix.iter().all(|v| component(v))
            })
        {
            return Err("invalid configured state policy".into());
        }
        for (name, diagnostic) in [
            ("NO_STATE", &self.missing_state),
            ("CONFIG_FORMAT", &self.malformed),
            ("CONFIG_SIZE", &self.oversized),
            ("CONFIG_CONFLICT", &self.conflict),
            ("CONFIG_RELAY_COUNT", &self.missing_relays),
        ] {
            if diagnostic.exit == 0
                || diagnostic.stderr.is_empty()
                || diagnostic.stderr.chars().any(char::is_control)
            {
                return Err("invalid configured state diagnostic".into());
            }
            writeln!(
                out,
                "const {name}: Failure = Failure({}, {:?});",
                diagnostic.exit, diagnostic.stderr
            )
            .unwrap();
        }
        writeln!(out, "const CONFIGURED_LEAF: &str = {:?};\nconst CONFIGURED_MAX: usize = {};\nconst CONFIG_RELAY_FIELD: &str = {:?};\nconst CONFIG_ROUTE_FIELD: &str = {:?};\nconst CONFIG_FIXED: &str = {:?};",
            self.leaf, self.maximum, self.relays, self.route, serde_json::to_string(&self.fixed).unwrap()).unwrap();
        writeln!(out, "const AUTO_BACKEND: &str = {:?};\nconst DEFAULT_ROUTE: &str = {:?};\nconst DIRECT_ROUTES: &[&str] = &{:?};\nconst MAXIMUM_RELAYS: usize = {};",
            self.auto_backend, self.default_route, self.direct_routes, self.maximum_relays).unwrap();
        self.relay_rules.render(out)?;
        writeln!(out, "const STATE_SOURCES: &[(&str,bool,bool,&[&str])] = &[").unwrap();
        for source in &self.sources {
            let platform = match source.platform {
                Some(Platform::Linux) => "cfg!(target_os = \"linux\")",
                Some(Platform::Macos) => "cfg!(target_os = \"macos\")",
                None => "true",
            };
            writeln!(
                out,
                "({:?},{platform},{},&{:?}),",
                source.variable, source.require_absolute, source.suffix
            )
            .unwrap();
        }
        out.push_str("];\n");
        let (check, relay_check) = if provisioning {
            (
                "validate_provision_fields(&document, &fixed)?;",
                "return canonical_provision_relays(route, relays);",
            )
        } else {
            ("if document != fixed { return Err(CONFIGURATION); }", "")
        };
        let template = include_str!("configured.rs.in").replace("@CHECK_FIELDS@", check);
        let direct = if provisioning {
            "configured_direct_relays"
        } else {
            "configured_relays"
        };
        out.push_str(&template.replace("@DIRECT_RELAY_CHECK@", direct));
        if provisioning {
            writeln!(out, "fn configured_relays(route: &str, relays: &[String]) -> Result<Vec<String>, Failure> {{ {relay_check} }}").unwrap();
        }
        Ok(())
    }
}

impl RelayRules {
    fn render(&self, out: &mut String) -> Result<(), String> {
        if !(1..=65536).contains(&self.maximum_bytes)
            || !distinct(self.rejected_host_suffixes.iter().map(String::as_str))
            || self
                .rejected_host_suffixes
                .iter()
                .any(|suffix| suffix.chars().any(char::is_control))
            || self.rejected_characters.contains('\0')
        {
            return Err("invalid configured URL admission policy".into());
        }
        writeln!(out, "const RELAY_MAX_BYTES: usize = {};\nconst REJECTED_HOST_SUFFIXES: &[&str] = &{:?};\nconst REJECTED_RELAY_CHARS: &str = {:?};",
            self.maximum_bytes, self.rejected_host_suffixes, self.rejected_characters).unwrap();
        for (name, flag) in [
            ("REJECT_CONTROL", self.reject_control),
            ("REJECT_WHITESPACE", self.reject_whitespace),
            ("REJECT_CREDENTIALS", self.reject_credentials),
            ("REJECT_QUERY", self.reject_query),
            ("REJECT_FRAGMENT", self.reject_fragment),
            ("REJECT_ZERO_PORT", self.reject_zero_port),
            ("REJECT_EMPTY_PORT", self.reject_empty_port),
            ("REJECT_PADDED_PORT", self.reject_padded_port),
        ] {
            writeln!(out, "const {name}: bool = {flag};").unwrap();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternate_url_admission_is_rendered_from_policy() {
        let graph = serde_json::json!({
            "maximum_bytes": 214,
            "rejected_host_suffixes": [".internal.test"],
            "rejected_characters": "~",
            "reject_control": true,
            "reject_whitespace": false,
            "reject_credentials": true,
            "reject_query": false,
            "reject_fragment": true,
            "reject_zero_port": false,
            "reject_empty_port": false,
            "reject_padded_port": false
        });
        let rules: RelayRules = serde_json::from_value(graph).unwrap();
        let mut rendered = String::new();
        rules.render(&mut rendered).unwrap();
        assert!(rendered.contains("214"));
        assert!(rendered.contains(r#"".internal.test""#));
        assert!(rendered.contains("REJECT_WHITESPACE: bool = false"));
        assert!(rendered.contains("REJECT_CREDENTIALS: bool = true"));
    }
}
