//! Resolve public type spellings without expanding potentially cyclic module paths.
use super::export_model::*;
use super::identity;
use rustc_hir::def::{DefKind, Namespace, Res};
use rustc_middle::metadata::ModChild;
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::{DefId, LOCAL_CRATE};
use std::collections::{BTreeMap, BTreeSet, HashSet};

fn fields(tcx: TyCtxt<'_>, variant: &ty::VariantDef) -> BTreeMap<String, Field> {
    variant
        .fields
        .iter()
        .map(|f| {
            let audience = match tcx.visibility(f.did) {
                ty::Visibility::Public => Audience::Public,
                ty::Visibility::Restricted(scope) => Audience::Scope {
                    id: identity(tcx, scope),
                },
            };
            (
                f.name.to_string(),
                Field {
                    id: identity(tcx, f.did),
                    audience,
                },
            )
        })
        .collect()
}

fn data(tcx: TyCtxt<'_>, id: DefId) -> Data {
    let adt = tcx.adt_def(id);
    if adt.is_enum() {
        let variants = adt
            .variants()
            .iter()
            .map(|v| {
                (
                    v.name.to_string(),
                    Variant {
                        id: identity(tcx, v.def_id),
                        fields: fields(tcx, v),
                    },
                )
            })
            .collect();
        Data {
            variants,
            ..Default::default()
        }
    } else {
        Data {
            fields: fields(tcx, adt.non_enum_variant()),
            ..Default::default()
        }
    }
}

fn binding(tcx: TyCtxt<'_>, child: &ModChild) -> Option<Binding> {
    match child.res {
        Res::Def(kind, target) => {
            let kind = if target.is_local() {
                match kind {
                    DefKind::Mod => Kind::Module,
                    DefKind::Struct | DefKind::Enum | DefKind::Union => Kind::Data,
                    DefKind::Trait | DefKind::TraitAlias => Kind::Trait,
                    DefKind::TyAlias => Kind::Alias,
                    DefKind::Variant => Kind::Variant,
                    _ => return None,
                }
            } else {
                Kind::External
            };
            Some(Binding {
                kind,
                target: identity(tcx, target),
            })
        }
        Res::PrimTy(primitive) => Some(Binding {
            kind: Kind::Primitive,
            target: primitive.name().to_string(),
        }),
        _ => None,
    }
}

fn candidate(tcx: TyCtxt<'_>, child: &ModChild) -> Option<Candidate> {
    let audience = match child.vis {
        ty::Visibility::Public => Audience::Public,
        ty::Visibility::Restricted(scope) => Audience::Scope {
            id: identity(tcx, scope),
        },
    };
    Some(Candidate {
        binding: binding(tcx, child)?,
        audience,
    })
}

pub(super) fn observe(tcx: TyCtxt<'_>, compiler: String) -> Observation {
    let root = LOCAL_CRATE.as_def_id();
    let mut graph = Graph {
        vocabulary: VOCABULARY.into(),
        root: identity(tcx, root),
        modules: BTreeMap::new(),
        data: BTreeMap::new(),
    };
    let mut pending = vec![root.expect_local()];
    let mut visited = HashSet::new();
    while let Some(module) = pending.pop() {
        if !visited.insert(module) {
            continue;
        }
        let mut row = Module::default();
        for ambiguity in tcx
            .resolutions(())
            .ambig_module_children
            .get(&module)
            .into_iter()
            .flatten()
        {
            let head = &ambiguity.main;
            if head.res.ns() != Some(Namespace::TypeNS)
                || !(head.vis.is_public() || ambiguity.second.vis.is_public())
            {
                continue;
            }
            let name = head.ident.name.to_string();
            if name == "_" {
                row.unresolved.insert(name);
                continue;
            }
            if row.unresolved.contains(&name) {
                continue;
            }
            let Some(primary) = candidate(tcx, head) else {
                row.ambiguous.remove(&name);
                row.unresolved.insert(name);
                continue;
            };
            let Some(other) = candidate(tcx, &ambiguity.second) else {
                row.ambiguous.remove(&name);
                row.unresolved.insert(name);
                continue;
            };
            match row.ambiguous.entry(name.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Ambiguity {
                        primary,
                        alternatives: BTreeSet::from([other]),
                    });
                }
                std::collections::btree_map::Entry::Occupied(mut entry)
                    if entry.get().primary == primary =>
                {
                    entry.get_mut().alternatives.insert(other);
                }
                std::collections::btree_map::Entry::Occupied(entry) => {
                    // One textual spelling denotes multiple incompatible hygiene contexts.
                    entry.remove();
                    row.unresolved.insert(name);
                }
            }
        }
        let mut targets = BTreeMap::new();
        for child in tcx.module_children_local(module) {
            if !child.vis.is_public() || child.res.ns() != Some(Namespace::TypeNS) {
                continue;
            }
            let name = child.ident.name.to_string();
            if name == "_" {
                row.unresolved.insert(name);
                continue;
            }
            if row.ambiguous.contains_key(&name) || row.unresolved.contains(&name) {
                continue;
            }
            let Some(value) = binding(tcx, child) else {
                row.bindings.remove(&name);
                row.unresolved.insert(name);
                continue;
            };
            if let Some(other) = row.bindings.insert(name.clone(), value)
                && row.bindings[&name] != other
            {
                row.bindings.remove(&name);
                row.unresolved.insert(name);
                continue;
            }
            if let Res::Def(_, target) = child.res {
                targets.insert(name, target);
            }
        }
        for (name, value) in &row.bindings {
            if let Some(&target) = targets.get(name) {
                match value.kind {
                    Kind::Module => pending.push(target.expect_local()),
                    Kind::Data => {
                        graph
                            .data
                            .entry(value.target.clone())
                            .or_insert_with(|| data(tcx, target));
                    }
                    _ => (),
                }
            }
        }
        graph.modules.insert(identity(tcx, module.to_def_id()), row);
    }
    for (id, path) in graph.canonical_paths() {
        graph.modules.get_mut(&id).unwrap().path = path;
    }
    Observation {
        compiler,
        crate_name: tcx.crate_name(LOCAL_CRATE).to_string(),
        graph,
    }
}
