use super::{Binding, Encoding, Graph, Result, composite::Primitive};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct PinnedRecord {
    graph: Graph,
    binding: Binding,
    paths: BTreeSet<String>,
}

impl PinnedRecord {
    pub(crate) fn read(
        graph: &[u8],
        overlay: &[u8],
        exports: impl FnOnce(&str) -> Vec<Vec<String>>,
    ) -> Result<Self> {
        let graph: Graph = serde_json::from_slice(graph).map_err(|e| e.to_string())?;
        let binding: Binding = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
        graph.validate(&binding)?;
        let paths = exports(&binding.name)
            .into_iter()
            .map(|path| format!("crate::{}", path.join("::")))
            .collect();
        Ok(Self {
            graph,
            binding,
            paths,
        })
    }

    fn member(&self, identity: &str) -> Option<(&str, Primitive)> {
        let slot = self.graph.layout.iter().find(|slot| slot.id == identity)?;
        Some((
            self.binding.names.get(identity)?,
            Primitive::from_encoding(&slot.encoding)?,
        ))
    }
}

pub(crate) fn validate(records: &[PinnedRecord]) -> Result<()> {
    let mut providers = BTreeMap::<&str, Vec<usize>>::new();
    for (index, record) in records.iter().enumerate() {
        // Existing-type fragments add prefix helpers. They do not declare an independent codec.
        if record.binding.existing_type.is_none() {
            providers.entry(&record.graph.id).or_default().push(index);
        }
    }
    let mut dependent = vec![Vec::new(); records.len()];
    let mut unresolved = vec![0; records.len()];
    for (index, consumer) in records.iter().enumerate() {
        for slot in &consumer.graph.layout {
            let Encoding::Nested {
                record, maximum, ..
            } = &slot.encoding
            else {
                continue;
            };
            let provider_index = match providers.get(record.as_str()).map(Vec::as_slice) {
                Some([index]) => *index,
                None => return Err(format!("{}: missing pinned nested codec {record}", slot.id)),
                Some(_) => {
                    return Err(format!(
                        "{}: ambiguous pinned nested codec {record}",
                        slot.id
                    ));
                }
            };
            let provider = &records[provider_index];
            if *maximum < provider.graph.maximum {
                return Err(format!(
                    "{}: nested slot maximum {maximum} is below pinned codec {record} maximum {}",
                    slot.id, provider.graph.maximum
                ));
            }
            let native = &consumer.binding.external_records[record];
            if !provider.paths.contains(&native.path) {
                return Err(format!(
                    "{}: nested codec path mismatch for {record}: {}",
                    slot.id, native.path
                ));
            }
            for (member, shape) in &consumer.graph.external_records[record].fields {
                if provider.member(member) != Some((native.fields[member].as_str(), *shape))
                {
                    return Err(format!(
                        "{}: nested codec member mismatch for {record}: {member}",
                        slot.id
                    ));
                }
            }
            unresolved[index] += 1;
            dependent[provider_index].push(index);
        }
    }

    let mut ready: Vec<_> = (0..records.len())
        .filter(|index| unresolved[*index] == 0)
        .collect();
    while let Some(index) = ready.pop() {
        for &consumer in &dependent[index] {
            unresolved[consumer] -= 1;
            if unresolved[consumer] == 0 {
                ready.push(consumer);
            }
        }
    }
    if let Some(index) = unresolved.iter().position(|count| *count != 0) {
        return Err(format!(
            "cyclic pinned nested codecs prevent resolving {}",
            records[index].graph.id
        ));
    }
    Ok(())
}
