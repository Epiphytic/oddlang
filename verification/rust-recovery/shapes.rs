//! Ordered, typed ADT observations from the selected compiler build.
use super::{BTreeSet, identity};
use rustc_hir::def::{CtorKind, DefKind};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::{
    def_id::{DefId, LOCAL_CRATE},
    sym,
};

use super::shape_model::*;

struct Collector<'tcx> {
    tcx: TyCtxt<'tcx>,
    gaps: BTreeSet<Gap>,
}

impl<'tcx> Collector<'tcx> {
    fn gap(&mut self, owner: &str, reason: &'static str) {
        self.gaps.insert(Gap {
            owner: owner.to_owned(),
            reason: reason.to_owned(),
        });
    }

    fn unresolved(&mut self, owner: &str, reason: &'static str) -> Shape {
        self.gap(owner, reason);
        Shape::Unresolved {
            reason: reason.to_owned(),
        }
    }

    fn parameter(&self, context: DefId, index: u32) -> Shape {
        let param = self
            .tcx
            .generics_of(context)
            .param_at(index as usize, self.tcx);
        Shape::Parameter {
            id: identity(self.tcx, param.def_id),
        }
    }

    fn lifetime(&mut self, owner: &str, context: DefId, region: ty::Region<'tcx>) -> Shape {
        match region.kind() {
            ty::ReStatic => Shape::StaticLifetime,
            ty::ReEarlyParam(param) => self.parameter(context, param.index),
            _ => self.unresolved(owner, "lifetime-binding"),
        }
    }

    fn shape(&mut self, owner: &str, context: DefId, ty: Ty<'tcx>) -> Shape {
        use rustc_type_ir::TyKind::*;
        let tcx = self.tcx;
        let pointer_bits = tcx.data_layout.pointer_size().bits();
        match *ty.kind() {
            Bool => Shape::Boolean,
            Char => Shape::UnicodeScalar,
            Str => Shape::Text,
            Never => Shape::Never,
            Int(ty) => match ty.bit_width() {
                Some(bits) => Shape::SignedInteger { bits },
                None => Shape::TargetInteger {
                    bits: pointer_bits,
                    signed: true,
                },
            },
            Uint(ty) => match ty.bit_width() {
                Some(bits) => Shape::UnsignedInteger { bits },
                None => Shape::TargetInteger {
                    bits: pointer_bits,
                    signed: false,
                },
            },
            Float(ty) => Shape::FloatingPoint {
                bits: ty.bit_width(),
            },
            Adt(def, args) => {
                let def_id = def.did();
                if !def_id.is_local() {
                    self.gap(owner, "external-package-instance");
                }
                let ownership = if def.is_box() {
                    Some("exclusive".to_owned())
                } else if tcx.is_diagnostic_item(sym::Arc, def_id)
                    || tcx.is_diagnostic_item(sym::Rc, def_id)
                {
                    Some("shared".to_owned())
                } else {
                    None
                };
                let generics = tcx.generics_of(def_id);
                let arguments = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        let param = generics.param_at(index, tcx);
                        let value = match arg.kind() {
                            ty::GenericArgKind::Type(ty) => self.shape(owner, context, ty),
                            ty::GenericArgKind::Lifetime(region) => {
                                self.lifetime(owner, context, region)
                            }
                            ty::GenericArgKind::Const(_) => {
                                self.unresolved(owner, "constant-argument")
                            }
                        };
                        Argument {
                            parameter: identity(tcx, param.def_id),
                            name: param.name.to_string(),
                            position: param.index,
                            value,
                        }
                    })
                    .collect();
                Shape::Named {
                    id: identity(tcx, def_id),
                    arguments,
                    ownership,
                }
            }
            Param(param) => self.parameter(context, param.index),
            Tuple(elements) => Shape::Tuple {
                elements: elements
                    .iter()
                    .enumerate()
                    .map(|(position, ty)| TupleElement {
                        position,
                        shape: self.shape(owner, context, ty),
                    })
                    .collect(),
            },
            Ref(region, target, mutability) => Shape::Reference {
                ownership: if mutability.is_mut() {
                    "exclusive-borrow"
                } else {
                    "shared-borrow"
                }
                .to_owned(),
                lifetime: Box::new(self.lifetime(owner, context, region)),
                target: Box::new(self.shape(owner, context, target)),
            },
            RawPtr(target, mutability) => {
                self.gap(owner, "raw-pointer-provenance");
                Shape::RawPointer {
                    mutable: mutability.is_mut(),
                    target: Box::new(self.shape(owner, context, target)),
                }
            }
            Slice(element) => Shape::Slice {
                element: Box::new(self.shape(owner, context, element)),
            },
            Array(..) => self.unresolved(owner, "array-length"),
            Alias(..) => self.unresolved(owner, "type-alias-projection"),
            Dynamic(..) => self.unresolved(owner, "dynamic-type"),
            FnPtr(..) => self.unresolved(owner, "function-pointer"),
            _ => self.unresolved(owner, "unsupported-type"),
        }
    }

    fn fields(
        &mut self,
        context: DefId,
        variant: &ty::VariantDef,
        owner: &str,
        form: &Form,
    ) -> Vec<Field> {
        let tcx = self.tcx;
        variant
            .fields
            .iter()
            .enumerate()
            .map(|(position, field)| {
                let id = identity(tcx, field.did);
                let ty = tcx
                    .type_of(field.did)
                    .instantiate_identity()
                    .skip_normalization();
                Field {
                    shape: self.shape(&id, context, ty),
                    id,
                    owner: owner.to_owned(),
                    name: matches!(form, Form::Record).then(|| field.name.to_string()),
                    position,
                }
            })
            .collect()
    }

    fn declaration(&mut self, def: DefId) -> Declaration {
        let tcx = self.tcx;
        let id = identity(tcx, def);
        let adt = tcx.adt_def(def);
        let generics = tcx.generics_of(def);
        let parameters = generics
            .own_params
            .iter()
            .map(|param| Parameter {
                id: identity(tcx, param.def_id),
                name: param.name.to_string(),
                position: param.index,
                kind: match param.kind {
                    ty::GenericParamDefKind::Lifetime => "lifetime",
                    ty::GenericParamDefKind::Type { .. } => "type",
                    ty::GenericParamDefKind::Const { .. } => "constant",
                }
                .to_owned(),
            })
            .collect();
        if generics.count() != 0 {
            self.gap(&id, "generic-constraints-and-defaults");
        }
        if id.contains('#') {
            self.gap(&id, "compiler-disambiguated-identity");
        }
        let shape = if adt.is_enum() {
            let variants = adt
                .variants()
                .iter()
                .enumerate()
                .map(|(position, variant)| {
                    let variant_id = identity(tcx, variant.def_id);
                    let form = form(variant);
                    if matches!(variant.discr, ty::VariantDiscr::Explicit(_)) {
                        self.gap(&variant_id, "explicit-discriminant");
                    }
                    Variant {
                        fields: self.fields(def, variant, &variant_id, &form),
                        id: variant_id,
                        owner: id.clone(),
                        name: variant.name.to_string(),
                        position,
                        form,
                    }
                })
                .collect();
            DeclarationShape::Sum { variants }
        } else {
            let variant = adt.non_enum_variant();
            let form = form(variant);
            let fields = self.fields(def, variant, &id, &form);
            if adt.is_union() {
                self.gap(&id, "union-active-field");
                DeclarationShape::Union { fields }
            } else {
                DeclarationShape::Product { form, fields }
            }
        };
        Declaration {
            id,
            name: tcx.item_name(def).to_string(),
            parameters,
            shape,
        }
    }
}

fn form(variant: &ty::VariantDef) -> Form {
    match variant.ctor_kind() {
        Some(CtorKind::Const) => Form::Unit,
        Some(CtorKind::Fn) => Form::Tuple,
        None => Form::Record,
    }
}

pub(super) fn observe(tcx: TyCtxt<'_>, compiler: String) -> Observation {
    let mut collector = Collector {
        tcx,
        gaps: BTreeSet::new(),
    };
    let root = identity(tcx, LOCAL_CRATE.as_def_id());
    for reason in [
        "data-layout-and-representation",
        "visibility-and-export-paths",
        "workspace-package-identities",
    ] {
        collector.gap(&root, reason);
    }
    let mut declarations: Vec<_> = tcx
        .iter_local_def_id()
        .map(|def| def.to_def_id())
        .filter(|&def| {
            matches!(
                tcx.def_kind(def),
                DefKind::Struct | DefKind::Enum | DefKind::Union
            )
        })
        .map(|def| collector.declaration(def))
        .collect();
    declarations.sort_by(|left, right| left.id.cmp(&right.id));
    Observation {
        compiler,
        crate_name: tcx.crate_name(LOCAL_CRATE).to_string(),
        graph: Graph {
            vocabulary: VOCABULARY.to_owned(),
            declarations,
            gaps: collector.gaps,
        },
    }
}
