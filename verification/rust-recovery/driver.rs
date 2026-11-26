#![feature(rustc_private)]

extern crate rustc_ast;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;
extern crate rustc_type_ir;

mod shape_model;
mod shapes;
mod relations;
mod tagged_frame;

use std::collections::BTreeSet;
use std::{env, fs, process};

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::{DefKind, Res};
use rustc_hir::definitions::DefPathData;
use rustc_hir::{ItemKind, UseKind};
use rustc_interface::interface::Compiler;
use rustc_middle::metadata::Reexport;
use rustc_middle::mir::visit::{MutatingUseContext, NonMutatingUseContext, PlaceContext, Visitor};
use rustc_middle::mir::{self, ProjectionElem, TerminatorKind};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::{DefId, LOCAL_CRATE};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Inventory {
    compiler: String,
    crate_name: String,
    declarations: Vec<Declaration>,
    edges: BTreeSet<Edge>,
    gaps: BTreeSet<Gap>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Declaration {
    id: String,
    name: Option<String>,
    kind: &'static str,
    owner: Option<String>,
    visibility: String,
    macro_generated: bool,
    line: Option<usize>,
    column: Option<usize>,
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<Signature>,
}

#[derive(Serialize)]
struct Signature {
    abi: String,
    safety: &'static str,
    variadic: bool,
    inputs: Vec<TypeShape>,
    output: TypeShape,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum TypeShape {
    Primitive {
        name: String,
    },
    Definition {
        id: String,
        arguments: Vec<TypeShape>,
    },
    Parameter {
        index: u32,
    },
    Tuple {
        elements: Vec<TypeShape>,
    },
    Reference {
        mutable: bool,
        target: Box<TypeShape>,
    },
    Pointer {
        mutable: bool,
        target: Box<TypeShape>,
    },
    Slice {
        element: Box<TypeShape>,
    },
    Unknown,
}

#[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    from: String,
    relation: &'static str,
    to: String,
}

#[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct Gap {
    owner: String,
    kind: &'static str,
}

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || "_-'".contains(ch))
}

fn identity(tcx: TyCtxt<'_>, def: DefId) -> String {
    if def.is_crate_root() {
        let crate_name = tcx.crate_name(def.krate);
        if def.is_local() {
            return format!("local::{crate_name}");
        }
        let multiple = tcx
            .crates(())
            .iter()
            .filter(|candidate| tcx.crate_name(**candidate) == crate_name)
            .take(2)
            .count()
            > 1;
        return if multiple {
            let instance = tcx.stable_crate_id(def.krate).as_u64();
            format!("external::{crate_name}::{{crate-{instance:016x}}}")
        } else {
            format!("external::{crate_name}")
        };
    }

    use DefPathData::*;
    let mut path = identity(tcx, tcx.parent(def));
    let segment = tcx.def_key(def).disambiguated_data;
    let (name, namespace) = match segment.data {
        TypeNs(name) => (Some(name), "type"),
        ValueNs(name) => (Some(name), ""),
        MacroNs(name) => (Some(name), "macro"),
        LifetimeNs(name) => (Some(name), "lifetime"),
        Impl => (None, "impl"),
        Use => (None, "use"),
        ForeignMod => (None, "extern"),
        GlobalAsm => (None, "asm"),
        Closure => (None, "closure"),
        Ctor => (None, "ctor"),
        AnonConst => (None, "constant"),
        OpaqueTy => (None, "opaque"),
        OpaqueLifetime(_) => (None, "opaque-lifetime"),
        AnonAssocTy(_) => (None, "associated-type"),
        SyntheticCoroutineBody => (None, "coroutine"),
        NestedStatic => (None, "nested-static"),
        CrateRoot => unreachable!("handled crate root"),
    };
    if let Some(name) = name {
        let name = name.as_str();
        if identifier(name) {
            path.push_str("::");
            path.push_str(name);
        } else {
            path.push_str(&format!("::{{anonymous-name}}#{}", def.index.as_u32()));
        }
    }
    if !namespace.is_empty() {
        path.push_str("::{");
        path.push_str(namespace);
        path.push('}');
    }
    if segment.disambiguator != 0 {
        path.push('#');
        path.push_str(&segment.disambiguator.to_string());
    }
    path
}

fn declaration_kind(kind: DefKind) -> Option<&'static str> {
    Some(match kind {
        DefKind::Mod => "module",
        DefKind::Struct => "struct",
        DefKind::Union => "union",
        DefKind::Enum => "enum",
        DefKind::Variant => "variant",
        DefKind::Trait => "trait",
        DefKind::TyAlias => "type-alias",
        DefKind::TraitAlias => "trait-alias",
        DefKind::ForeignTy => "foreign-type",
        DefKind::AssocTy => "associated-type",
        DefKind::TyParam => "type-parameter",
        DefKind::Fn => "function",
        DefKind::Const { .. } => "constant",
        DefKind::ConstParam => "const-parameter",
        DefKind::Static { nested: false, .. } => "static",
        DefKind::Ctor(..) => "constructor",
        DefKind::AssocFn => "associated-function",
        DefKind::AssocConst { .. } => "associated-constant",
        DefKind::Macro(_) => "macro",
        DefKind::ExternCrate => "extern-crate",
        DefKind::Use => "import",
        DefKind::ForeignMod => "foreign-module",
        DefKind::OpaqueTy => "opaque-type",
        DefKind::Field => "field",
        DefKind::LifetimeParam => "lifetime-parameter",
        DefKind::GlobalAsm => "global-assembly",
        DefKind::Impl { .. } => "implementation",
        DefKind::Closure => "closure",
        DefKind::SyntheticCoroutineBody
        | DefKind::Static { nested: true, .. }
        | DefKind::AnonConst
        | DefKind::InlineConst => return None,
    })
}

fn recorded_parent(tcx: TyCtxt<'_>, def: DefId) -> Option<DefId> {
    let mut parent = tcx.opt_parent(def);
    while let Some(candidate) = parent {
        if declaration_kind(tcx.def_kind(candidate)).is_some() {
            return Some(candidate);
        }
        parent = tcx.opt_parent(candidate);
    }
    None
}

struct Collector<'tcx> {
    tcx: TyCtxt<'tcx>,
    report: Inventory,
}

impl<'tcx> Collector<'tcx> {
    fn edge(&mut self, owner: &str, relation: &'static str, target: DefId) {
        self.report.edges.insert(Edge {
            from: owner.to_owned(),
            relation,
            to: identity(self.tcx, target),
        });
    }

    fn gap(&mut self, owner: &str, kind: &'static str) {
        self.report.gaps.insert(Gap {
            owner: owner.to_owned(),
            kind,
        });
    }

    fn shape(&mut self, owner: &str, ty: Ty<'tcx>) -> TypeShape {
        use rustc_type_ir::TyKind::*;
        match *ty.kind() {
            Bool | Char | Int(_) | Uint(_) | Float(_) | Str | Never => TypeShape::Primitive {
                name: ty.to_string(),
            },
            Adt(def, args) => {
                let id = def.did();
                self.edge(owner, "signature-type", id);
                if args.iter().any(|arg| arg.as_const().is_some()) {
                    self.gap(owner, "signature-const-arguments");
                }
                let arguments = args.types().map(|ty| self.shape(owner, ty)).collect();
                TypeShape::Definition {
                    id: identity(self.tcx, id),
                    arguments,
                }
            }
            Param(param) => TypeShape::Parameter { index: param.index },
            Tuple(elements) => TypeShape::Tuple {
                elements: elements.iter().map(|ty| self.shape(owner, ty)).collect(),
            },
            Ref(_, ty, mutability) => {
                self.gap(owner, "signature-lifetimes");
                TypeShape::Reference {
                    mutable: mutability.is_mut(),
                    target: Box::new(self.shape(owner, ty)),
                }
            }
            RawPtr(ty, mutability) => TypeShape::Pointer {
                mutable: mutability.is_mut(),
                target: Box::new(self.shape(owner, ty)),
            },
            Slice(ty) => TypeShape::Slice {
                element: Box::new(self.shape(owner, ty)),
            },
            Dynamic(predicates, _) => {
                for predicate in predicates.iter() {
                    if let ty::ExistentialPredicate::Trait(trait_ref) = predicate.skip_binder() {
                        self.edge(owner, "signature-type", trait_ref.def_id);
                    }
                }
                self.gap(owner, "signature-type-shape");
                TypeShape::Unknown
            }
            _ => {
                self.gap(owner, "signature-type-shape");
                TypeShape::Unknown
            }
        }
    }

    fn declaration(&mut self, def: DefId) {
        let tcx = self.tcx;
        let def_kind = tcx.def_kind(def);
        let Some(kind) = declaration_kind(def_kind) else {
            return;
        };
        let id = identity(tcx, def);
        let parent = tcx.opt_parent(def);
        let owner = recorded_parent(tcx, def).map(|owner| identity(tcx, owner));
        if matches!(
            def_kind,
            DefKind::Impl { .. } | DefKind::Use | DefKind::Closure
        ) || tcx.def_key(def).disambiguated_data.disambiguator != 0
        {
            self.gap(&id, "ordinal-identity");
        }
        let visibility = if !matches!(
            def_kind,
            DefKind::Impl { .. }
                | DefKind::Closure
                | DefKind::TyParam
                | DefKind::ConstParam
                | DefKind::LifetimeParam
                | DefKind::GlobalAsm
                | DefKind::ForeignMod
                | DefKind::OpaqueTy
        ) {
            match tcx.visibility(def) {
                ty::Visibility::Public => "public".to_owned(),
                ty::Visibility::Restricted(scope) if Some(scope) == parent => "private".to_owned(),
                ty::Visibility::Restricted(scope) if scope.is_crate_root() => "crate".to_owned(),
                ty::Visibility::Restricted(scope) => {
                    self.edge(&id, "visibility-scope", scope);
                    "restricted".to_owned()
                }
            }
        } else {
            "inherited".to_owned()
        };
        let span = tcx.def_span(def);
        let source_map = tcx.sess.source_map();
        let position = (!span.is_dummy()).then(|| source_map.lookup_char_pos(span.lo()));
        let name = if def_kind == DefKind::Use {
            match tcx.hir_expect_item(def.expect_local()).kind {
                ItemKind::Use(_, UseKind::Single(ident)) => Some(ident.name.to_string()),
                _ => None,
            }
        } else {
            tcx.opt_item_name(def).map(|name| name.to_string())
        };
        let name = name.filter(|name| {
            if identifier(name) {
                return true;
            }
            self.gap(&id, "unresolved-identity");
            false
        });
        if id.contains("::{anonymous-name}") {
            self.gap(&id, "unresolved-identity");
        }
        let signature = if matches!(def_kind, DefKind::Fn | DefKind::AssocFn) {
            let sig = tcx.fn_sig(def).instantiate_identity().skip_binder();
            Some(Signature {
                abi: sig.abi().as_str().to_owned(),
                safety: if sig.safety().is_unsafe() {
                    "unsafe"
                } else {
                    "safe"
                },
                variadic: sig.c_variadic(),
                inputs: sig.inputs().iter().map(|ty| self.shape(&id, *ty)).collect(),
                output: self.shape(&id, sig.output()),
            })
        } else {
            if matches!(
                def_kind,
                DefKind::Field
                    | DefKind::Const { .. }
                    | DefKind::Static { .. }
                    | DefKind::TyAlias
                    | DefKind::AssocConst { .. }
            ) {
                self.shape(
                    &id,
                    tcx.type_of(def).instantiate_identity().skip_normalization(),
                );
                self.gap(&id, "typed-declaration-shape");
            }
            None
        };
        if matches!(
            def_kind,
            DefKind::Fn
                | DefKind::AssocFn
                | DefKind::Impl { .. }
                | DefKind::Struct
                | DefKind::Union
                | DefKind::Enum
                | DefKind::Trait
                | DefKind::TyAlias
                | DefKind::TraitAlias
                | DefKind::AssocTy
        ) {
            let predicates = tcx.explicit_predicates_of(def);
            for (clause, _) in predicates.predicates {
                if let ty::ClauseKind::Trait(bound) = clause.kind().skip_binder() {
                    self.edge(&id, "generic-bound", bound.trait_ref.def_id);
                }
            }
        }
        if matches!(def_kind, DefKind::Impl { .. }) {
            self.shape(
                &id,
                tcx.type_of(def).instantiate_identity().skip_normalization(),
            );
            self.gap(&id, "typed-declaration-shape");
        }
        if let DefKind::Impl { of_trait: true } = def_kind {
            self.edge(
                &id,
                "implements",
                tcx.impl_trait_ref(def)
                    .instantiate_identity()
                    .skip_normalization()
                    .def_id,
            );
        }
        if def_kind == DefKind::Mod {
            for child in tcx.module_children_local(def.expect_local()) {
                if let (Res::Def(_, target), Some(reexport)) =
                    (child.res, child.reexport_chain.first())
                {
                    if let Some(import) = reexport.id().filter(|import| import.is_local()) {
                        let import = identity(tcx, import);
                        self.edge(&import, "reexport", target);
                        if matches!(reexport, Reexport::Glob(_)) {
                            self.gap(&import, "reexport-alias");
                        }
                    } else {
                        self.edge(&id, "reexport", target);
                        self.gap(&id, "reexport-alias");
                    }
                }
            }
        }
        self.report.declarations.push(Declaration {
            id,
            name,
            kind: if def.is_crate_root() { "crate" } else { kind },
            owner,
            visibility,
            macro_generated: span.from_expansion(),
            line: position.as_ref().map(|loc| loc.line),
            column: position.as_ref().map(|loc| loc.col.0 + 1),
            file: position
                .as_ref()
                .map(|loc| loc.file.name.prefer_remapped_unconditionally().to_string()),
            signature,
        });
    }
}

struct BodyVisitor<'a, 'tcx> {
    collector: &'a mut Collector<'tcx>,
    body: &'a mir::Body<'tcx>,
    def: DefId,
    owner: String,
}

impl<'tcx> Visitor<'tcx> for BodyVisitor<'_, 'tcx> {
    fn visit_terminator(&mut self, terminator: &mir::Terminator<'tcx>, location: mir::Location) {
        let tcx = self.collector.tcx;
        if let TerminatorKind::Call { func, .. } | TerminatorKind::TailCall { func, .. } =
            &terminator.kind
        {
            if let ty::FnDef(callee, args) = *func.ty(&self.body.local_decls, tcx).kind() {
                if tcx.trait_of_assoc(callee).is_some() {
                    self.collector.edge(&self.owner, "dispatch", callee);
                    let env = ty::TypingEnv::post_analysis(tcx, self.def);
                    match ty::Instance::try_resolve(tcx, env, callee, args) {
                        Ok(Some(instance)) => match instance.def {
                            ty::InstanceKind::Item(target) => {
                                self.collector.edge(&self.owner, "call", target)
                            }
                            ty::InstanceKind::Virtual(..) => {
                                self.collector.gap(&self.owner, "dynamic-dispatch")
                            }
                            _ => self.collector.gap(&self.owner, "compiler-shim-dispatch"),
                        },
                        _ => self.collector.gap(&self.owner, "generic-dispatch"),
                    }
                } else {
                    self.collector.edge(&self.owner, "call", callee);
                }
            } else {
                self.collector.gap(&self.owner, "indirect-call");
            }
        }
        if matches!(terminator.kind, TerminatorKind::Drop { .. }) {
            self.collector.gap(&self.owner, "implicit-drop");
        }
        self.super_terminator(terminator, location);
    }

    fn visit_rvalue(&mut self, rvalue: &mir::Rvalue<'tcx>, location: mir::Location) {
        if let mir::Rvalue::Aggregate(kind, _) = rvalue {
            if let mir::AggregateKind::Adt(def, index, _, _, active) = **kind {
                let variant = self.collector.tcx.adt_def(def).variant(index);
                for (field_index, field) in variant.fields.iter_enumerated() {
                    if active.is_none_or(|active| active == field_index) {
                        self.collector.edge(&self.owner, "write", field.did);
                    }
                }
            }
        }
        self.super_rvalue(rvalue, location);
    }

    fn visit_place(
        &mut self,
        place: &mir::Place<'tcx>,
        context: PlaceContext,
        location: mir::Location,
    ) {
        let tcx = self.collector.tcx;
        let relation = match context {
            PlaceContext::MutatingUse(
                MutatingUseContext::Store
                | MutatingUseContext::Call
                | MutatingUseContext::AsmOutput,
            ) => Some("write"),
            PlaceContext::NonMutatingUse(
                NonMutatingUseContext::Copy
                | NonMutatingUseContext::Move
                | NonMutatingUseContext::Inspect,
            ) => Some("read"),
            PlaceContext::MutatingUse(
                MutatingUseContext::Borrow | MutatingUseContext::RawBorrow,
            ) => Some("mutable-borrow"),
            PlaceContext::NonMutatingUse(
                NonMutatingUseContext::SharedBorrow | NonMutatingUseContext::RawBorrow,
            ) => Some("shared-borrow"),
            _ => None,
        };
        if let Some(relation) = relation {
            if place
                .iter_projections()
                .rev()
                .skip(1)
                .any(|(_, projection)| matches!(projection, ProjectionElem::Field(..)))
            {
                self.collector.gap(&self.owner, "nested-field-projection");
            }
            if let Some((parent, ProjectionElem::Field(field, _))) =
                place.as_ref().last_projection()
            {
                let parent = parent.ty(&self.body.local_decls, tcx);
                if let ty::Adt(adt, _) = parent.ty.kind() {
                    let definition = match parent.variant_index {
                        Some(index) => adt.variant(index),
                        None => adt.non_enum_variant(),
                    };
                    self.collector
                        .edge(&self.owner, relation, definition.fields[field].did);
                }
            }
        }
        self.super_place(place, context, location);
    }

    fn visit_operand(&mut self, operand: &mir::Operand<'tcx>, location: mir::Location) {
        let tcx = self.collector.tcx;
        if let ty::FnDef(def, _) = *operand.ty(&self.body.local_decls, tcx).kind() {
            self.collector.edge(&self.owner, "function-reference", def);
        }
        self.super_operand(operand, location);
    }

    fn visit_const_operand(&mut self, operand: &mir::ConstOperand<'tcx>, location: mir::Location) {
        let tcx = self.collector.tcx;
        if let Some(def) = operand.check_static_ptr(tcx) {
            self.collector.edge(&self.owner, "static-reference", def);
            self.collector
                .gap(&self.owner, "static-access-classification");
        }
        if let mir::Const::Unevaluated(value, _) = operand.const_ {
            if matches!(
                tcx.def_kind(value.def),
                DefKind::Const { .. } | DefKind::AssocConst { .. }
            ) {
                self.collector.edge(&self.owner, "read", value.def);
            }
        }
        self.super_const_operand(operand, location);
    }
}

struct Recovery;

impl Callbacks for Recovery {
    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, tcx: TyCtxt<'tcx>) -> Compilation {
        if let Ok(selection) = env::var("ODD_RECOVERY_TAGGED_FRAME_GUARDS") {
            let compiler = env::var("ODD_RECOVERY_COMPILER").expect("compiler commit");
            let output = env::var_os("ODD_RECOVERY_OUT").expect("frame guard destination");
            let file = fs::File::create_new(output).expect("create frame guard observation");
            serde_json::to_writer(file, &tagged_frame::observe_guards(tcx, &compiler, &selection))
                .expect("serialize frame guard observation");
            return Compilation::Continue;
        }
        if let Ok(method) = env::var("ODD_RECOVERY_TAGGED_FRAME") {
            let compiler = env::var("ODD_RECOVERY_COMPILER").expect("compiler commit");
            let output = env::var_os("ODD_RECOVERY_OUT").expect("tagged frame destination");
            let file = fs::File::create_new(output).expect("create tagged frame observation");
            serde_json::to_writer(file, &tagged_frame::observe(tcx, &compiler, &method))
                .expect("serialize tagged frame observation");
            return Compilation::Continue;
        }
        if let Ok(method) = env::var("ODD_RECOVERY_FIELD_CONJUNCTION") {
            let compiler = env::var("ODD_RECOVERY_COMPILER").expect("compiler commit");
            let output = env::var_os("ODD_RECOVERY_OUT").expect("field conjunction destination");
            let file = fs::File::create_new(output).expect("create field conjunction observation");
            serde_json::to_writer(file, &relations::observe(tcx, &compiler, &method))
                .expect("serialize field conjunction observation");
            return Compilation::Continue;
        }
        if env::var_os("ODD_RECOVERY_DATA_SHAPES").is_some() {
            let compiler = env::var("ODD_RECOVERY_COMPILER").expect("compiler commit");
            let output = env::var_os("ODD_RECOVERY_OUT").expect("shape observation destination");
            let file = fs::File::create_new(output).expect("create shape observation");
            serde_json::to_writer(file, &shapes::observe(tcx, compiler))
                .expect("serialize shape observation");
            return Compilation::Continue;
        }
        let mut collector = Collector {
            tcx,
            report: Inventory {
                compiler: env::var("ODD_RECOVERY_COMPILER").expect("compiler commit"),
                crate_name: tcx.crate_name(LOCAL_CRATE).to_string(),
                declarations: Vec::new(),
                edges: BTreeSet::new(),
                gaps: BTreeSet::new(),
            },
        };
        let root = identity(tcx, LOCAL_CRATE.as_def_id());
        for gap in [
            "body-coverage",
            "generic-bounds",
            "declaration-order",
            "package-instance-identities",
        ] {
            collector.gap(&root, gap);
        }
        for def in tcx.iter_local_def_id() {
            collector.declaration(def.to_def_id());
        }
        for &def in tcx.mir_keys(()) {
            let def_id = def.to_def_id();
            let kind = tcx.def_kind(def);
            let runtime = matches!(kind, DefKind::Fn | DefKind::AssocFn | DefKind::Closure);
            if !runtime
                && !matches!(
                    kind,
                    DefKind::Const { .. }
                        | DefKind::AssocConst { .. }
                        | DefKind::Static { nested: false, .. }
                )
            {
                if matches!(
                    kind,
                    DefKind::AnonConst | DefKind::InlineConst | DefKind::SyntheticCoroutineBody
                ) {
                    let owner = recorded_parent(tcx, def_id)
                        .expect("a nested body has a recorded ancestor");
                    collector.gap(&identity(tcx, owner), "skipped-body");
                }
                continue;
            }
            let owner = identity(tcx, def_id);
            let unoptimized = tcx.mir_drops_elaborated_and_const_checked(def);
            if unoptimized.is_stolen() {
                collector.gap(&owner, "body-mir-optimization");
                let body = if runtime {
                    tcx.optimized_mir(def_id)
                } else {
                    tcx.mir_for_ctfe(def_id)
                };
                BodyVisitor {
                    collector: &mut collector,
                    body,
                    def: def_id,
                    owner,
                }
                .visit_body(body);
            } else {
                let body = unoptimized.borrow();
                BodyVisitor {
                    collector: &mut collector,
                    body: &body,
                    def: def_id,
                    owner,
                }
                .visit_body(&body);
            }
        }
        collector
            .report
            .declarations
            .sort_by(|a, b| a.id.cmp(&b.id));
        let output = env::var_os("ODD_RECOVERY_OUT").expect("inventory destination");
        let file = fs::File::create_new(output).expect("create compiler inventory");
        serde_json::to_writer(file, &collector.report).expect("serialize compiler inventory");
        Compilation::Continue
    }
}

fn main() -> process::ExitCode {
    let mut args = env::args();
    let _wrapper = args.next();
    let mut args: Vec<_> = args.collect();
    let selected_package = env::var("ODD_RECOVERY_PACKAGE").ok();
    let selected_target = env::var("ODD_RECOVERY_TARGET")
        .unwrap_or_default()
        .replace('-', "_");
    let selected_kind = env::var("ODD_RECOVERY_KIND").unwrap_or_default();
    let target_matches = args
        .windows(2)
        .any(|pair| pair[0] == "--crate-name" && pair[1] == selected_target);
    let is_binary = env::var_os("CARGO_BIN_NAME").is_some();
    if selected_package.is_none()
        || env::var("CARGO_PKG_NAME").ok() != selected_package
        || !target_matches
        || (selected_kind == "bin") != is_binary
    {
        let compiler = args.remove(0);
        return process::Command::new(compiler)
            .args(args)
            .status()
            .map(|status| process::ExitCode::from(status.code().unwrap_or(1) as u8))
            .unwrap_or(process::ExitCode::FAILURE);
    }
    if !args
        .iter()
        .any(|arg| arg == "--sysroot" || arg.starts_with("--sysroot="))
    {
        args.push("--sysroot".to_owned());
        args.push(env::var("ODD_RECOVERY_SYSROOT").expect("compiler sysroot"));
    }
    rustc_driver::catch_with_exit_code(|| rustc_driver::run_compiler(&args, &mut Recovery))
}
