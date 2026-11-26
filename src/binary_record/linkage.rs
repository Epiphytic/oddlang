//! Check how a package's nested slots and concrete codec declarations fit together.
use super::{Binding, Encoding, Graph, Result, composite::Primitive};
use std::collections::BTreeMap;

type Aliases = BTreeMap<String, String>;

fn resolve_error_path<'a>(path: &'a str, aliases: &'a Aliases) -> &'a str {
    aliases.get(path).map(String::as_str).unwrap_or(path)
}

pub(crate) struct Record {
    graph: Graph,
    binding: Binding,
}

impl Record {
    pub(crate) fn read(graph: &[u8], overlay: &[u8]) -> Result<Self> {
        let graph: Graph = serde_json::from_slice(graph).map_err(|e| e.to_string())?;
        let binding: Binding = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
        // Each component has passed projection. The package rechecks both input digests
        // before providing these bytes for comparison.
        Ok(Self { graph, binding })
    }

    pub(crate) fn name(&self) -> &str {
        &self.binding.name
    }

    fn check_reference(&self, id: &str, provider: &Self, aliases: &Aliases) -> Result<()> {
        if provider.graph.id != id || provider.binding.existing_type.is_some() {
            return Err(format!("nested codec identity mismatch: {id}"));
        }
        if resolve_error_path(&self.binding.error_type, aliases)
            != resolve_error_path(&provider.binding.error_type, aliases)
        {
            return Err(format!("nested codec error type mismatch: {id}"));
        }
        let expected = &self.graph.external_records[id];
        if expected
            .codec_maximum
            .is_some_and(|limit| limit != provider.graph.maximum)
        {
            return Err(format!("nested codec maximum mismatch: {id}"));
        }
        for (member, kind) in &expected.fields {
            let slot = provider.graph.layout.iter().find(|slot| slot.id == *member);
            let actual = slot.and_then(|slot| Primitive::from_encoding(&slot.encoding));
            let name = &self.binding.external_records[id].fields[member];
            if actual.as_ref() != Some(kind) || provider.binding.names.get(member) != Some(name) {
                return Err(format!("nested codec member mismatch: {id} / {member}"));
            }
        }
        Ok(())
    }
}

pub(crate) fn validate(records: &[(Record, Vec<Vec<String>>)], aliases: &Aliases) -> Result<()> {
    let mut by_path = BTreeMap::new();
    for (index, (_, paths)) in records.iter().enumerate() {
        for path in paths {
            let path = format!("crate::{}", path.join("::"));
            if by_path.insert(path.clone(), index).is_some() {
                return Err(format!("ambiguous codec export: {path}"));
            }
        }
    }
    let mut dependencies = Vec::new();
    let mut minima = Vec::new();
    for (consumer, _) in records {
        let mut minimum = if consumer.graph.integrity.is_some() {
            32
        } else {
            0
        };
        let mut slots = Vec::new();
        for slot in &consumer.graph.layout {
            minimum += slot.encoding.width(&consumer.graph.records)?.0;
            if let Encoding::Nested {
                record, maximum, ..
            } = &slot.encoding
            {
                let path = &consumer.binding.external_records[record].path;
                let index = *by_path.get(path).ok_or_else(|| {
                    format!("nested codec identity has no pinned package export: {record} ({path})")
                })?;
                consumer.check_reference(record, &records[index].0, aliases)?;
                slots.push((index, *maximum, &slot.id));
            }
        }
        dependencies.push(slots);
        minima.push(minimum);
    }
    // Each record counts its own length prefixes and footer. Add each child’s
    // complete payload minimum once its dependencies are known.
    let mut complete = vec![false; records.len()];
    while complete.contains(&false) {
        let mut progress = false;
        for (index, slots) in dependencies.iter().enumerate() {
            if complete[index] || slots.iter().any(|(child, _, _)| !complete[*child]) {
                continue;
            }
            for (child, limit, id) in slots {
                if minima[*child] > u64::from(*limit) {
                    return Err(format!("nested codec minimum exceeds slot: {id}"));
                }
                minima[index] += minima[*child];
            }
            if minima[index] > u64::from(records[index].0.graph.maximum) {
                return Err(format!(
                    "nested codec minimum exceeds record: {}",
                    records[index].0.graph.id
                ));
            }
            complete[index] = true;
            progress = true;
        }
        if !progress {
            return Err("cyclic required nested codec slots".into());
        }
    }
    Ok(())
}
