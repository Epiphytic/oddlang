//! Structural comparison of a selected WIT world, addressed by semantic IRI.
use super::model::{Map, Res, World};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    OldHostNewGuest,
    NewHostOldGuest,
}
#[derive(Debug, Serialize)]
pub struct Change {
    pub iri: String,
    pub endpoint: String,
    pub old_surface: Vec<String>,
    pub new_surface: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub verdict: &'static str,
    pub direction: Direction,
    pub changes: Vec<Change>,
}

fn surfaces(w: &World) -> Res<BTreeMap<String, BTreeSet<String>>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // A world implicitly imports the interface that owns each used type. Include
    // its transitive interface uses, while excluding unrelated package interfaces.
    let links = w.effective()?;
    let mut pending: Vec<_> = links.values().flatten().cloned().collect();
    let mut reachable = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !reachable.insert(id.clone()) {
            continue;
        }
        let node = &w.nodes[&id];
        if node.kind == "Interface" {
            pending.extend(node.targets("use").cloned());
        } else if matches!(node.kind.as_str(), "Record" | "Variant" | "Enum") {
            pending.extend(
                w.nodes
                    .iter()
                    .filter(|(_, owner)| owner.targets("type").any(|t| t == &id))
                    .map(|(iri, _)| iri.clone()),
            );
        }
    }
    let interfaces: BTreeSet<_> = reachable
        .iter()
        .filter_map(|id| {
            let n = &w.nodes[id];
            (n.kind == "Interface").then_some(n.name.as_str())
        })
        .collect();
    for (path, id) in w.identities()? {
        if let Some(tail) = path.strip_prefix("interface/")
            && !interfaces.contains(tail.split('/').next().unwrap())
        {
            continue;
        }
        let n = &w.nodes[&id];
        let mut details = Map::new();
        for key in ["valueType", "result", "ordinal", "from"] {
            if let Some(v) = n.props.get(key) {
                let norm = match key {
                    "valueType" | "result" => super::wit::type_expr(v)?,
                    "ordinal" => v.parse::<usize>().map_err(|e| e.to_string())?.to_string(),
                    _ => v.clone(),
                };
                details.insert(key.into(), norm);
            }
        }
        let mut surface = format!("{path} {} {}", n.kind, n.name);
        for (k, v) in details {
            surface += &format!(" {k}={v}")
        }
        out.entry(id).or_default().insert(surface);
    }
    Ok(out)
}

pub fn compare(old: &World, new: &World, direction: Direction) -> Res<Report> {
    let old = surfaces(old)?;
    let new = surfaces(new)?;
    let mut changes = Vec::new();
    for id in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
        if old.get(id) == new.get(id) {
            continue;
        }
        let old_surface: Vec<_> = old.get(id).into_iter().flatten().cloned().collect();
        let new_surface: Vec<_> = new.get(id).into_iter().flatten().cloned().collect();
        let (host, guest) = match direction {
            Direction::OldHostNewGuest => (&old_surface, &new_surface),
            Direction::NewHostOldGuest => (&new_surface, &old_surface),
        };
        let requires_host = guest
            .iter()
            .any(|s| s.starts_with("world/import/") && !host.contains(s));
        let requires_guest = host
            .iter()
            .any(|s| s.starts_with("world/export/") && !guest.contains(s));
        let endpoint = match (requires_host, requires_guest) {
            (true, true) => "host-and-guest",
            (true, false) => "host",
            (false, true) => "guest",
            _ if host
                .iter()
                .chain(guest)
                .any(|s| s.starts_with("world/import/") || s.starts_with("world/export/")) =>
            {
                "none"
            }
            _ => "host-and-guest",
        };
        changes.push(Change {
            iri: id.clone(),
            endpoint: endpoint.into(),
            old_surface,
            new_surface,
        });
    }
    Ok(Report {
        verdict: if changes.is_empty() {
            "surface-equal"
        } else {
            "surface-changed"
        },
        direction,
        changes,
    })
}
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} ({:?})", self.verdict, self.direction)?;
        for c in &self.changes {
            writeln!(
                f,
                "{} [{}] {:?} -> {:?}",
                c.iri, c.endpoint, c.old_surface, c.new_surface
            )?
        }
        Ok(())
    }
}
