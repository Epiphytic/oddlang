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
    fixed: serde_json::Map<String, serde_json::Value>,
    sources: Vec<StateSource>,
    missing_state: Diagnostic,
    malformed: Diagnostic,
    oversized: Diagnostic,
    conflict: Diagnostic,
    pub(super) storage: super::schema::ConfiguredStorage,
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
    pub(super) fn render(&self, out: &mut String) -> Result<(), String> {
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
        out.push_str(include_str!("configured.rs.in"));
        Ok(())
    }
}
