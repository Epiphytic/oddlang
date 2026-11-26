//! Targeted compiler observation of a Rust field equality conjunction.
use rustc_hir::{
    BinOpKind, Expr, ExprKind, PatKind, QPath,
    def::{DefKind, Res},
};
use rustc_middle::{
    mir::{self, Operand, ProjectionElem, Rvalue, StatementKind, TerminatorKind},
    ty::{self, Ty, TyCtxt, TypeckResults},
};
use rustc_span::{
    Symbol,
    def_id::{DefId, LocalDefId},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

type Result<T> = std::result::Result<T, String>;

pub fn observe(tcx: TyCtxt<'_>, compiler: &str, selected: &str) -> Value {
    let mut evidence = json!({"format":"oddlang-rust-field-conjunction-v1","compiler":compiler,
                            "method":selected,"unresolved":[]});
    let result = find_method(tcx, selected).and_then(|def| {
        let span = tcx.def_span(def);
        if span.from_expansion() {
            return Err("a macro-generated method declaration is unresolved".into());
        }
        let position = tcx.sess.source_map().lookup_char_pos(span.lo());
        let root = std::env::var_os("CARGO_MANIFEST_DIR").ok_or("missing Cargo source root")?;
        let root = fs::canonicalize(root)
            .map_err(|err| format!("unavailable Cargo source root: {err}"))?;
        let (method_file, digest) = source_file(&position.file, &root)?;
        let mut inputs = BTreeMap::new();
        for file in tcx
            .sess
            .source_map()
            .files()
            .iter()
            .filter(|file| !file.is_imported())
        {
            if matches!(file.name, rustc_span::FileName::Real(_)) {
                let (path, digest) = source_file(file, &root)?;
                inputs.insert(path, digest);
            }
        }
        evidence["sourceFile"] = json!(method_file);
        evidence["sourceSha256"] = json!(digest);
        evidence["sourceFiles"] = json!(inputs);
        evidence["line"] = json!(position.line);
        evidence["comparisons"] = json!(inspect(tcx, def)?);
        Ok(())
    });
    if let Err(reason) = result {
        evidence["unresolved"] = json!([reason]);
    }
    evidence
}

fn source_file(file: &rustc_span::SourceFile, root: &Path) -> Result<(String, String)> {
    let path = file
        .name
        .clone()
        .into_local_path()
        .ok_or("missing compiler source file path")?;
    let path =
        fs::canonicalize(path).map_err(|err| format!("unavailable compiler source file: {err}"))?;
    let name = path
        .strip_prefix(root)
        .map_err(|_| "compiler source file lies outside the Cargo root")?
        .to_str()
        .ok_or("compiler source file path is not UTF-8")?;
    let raw = fs::read_to_string(&path)
        .map_err(|err| format!("cannot read compiler source file: {err}"))?;
    if !file.src_hash.matches(&raw) {
        return Err("compiler source file changed during observation".into());
    }
    let digest = Sha256::digest(raw.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((name.to_owned(), digest))
}

fn path(tcx: TyCtxt<'_>, def: DefId) -> Result<String> {
    if !def.is_local() {
        return Err("external record ownership is unproven".into());
    }
    if !matches!(tcx.def_kind(def), DefKind::Struct | DefKind::Enum) {
        return Err("compiler owner is not a nominal record".into());
    }
    let mut names = Vec::new();
    let mut cursor = def;
    while !cursor.is_crate_root() {
        let name = tcx
            .opt_item_name(cursor)
            .ok_or("anonymous compiler owner")?
            .to_string();
        if !super::identifier(&name) {
            return Err("unresolved compiler owner name".into());
        }
        names.push(name);
        cursor = tcx.opt_parent(cursor).ok_or("missing compiler parent")?;
        if !cursor.is_crate_root() && tcx.def_kind(cursor) != DefKind::Mod {
            return Err("compiler owner is not module-scoped".into());
        }
    }
    names.reverse();
    Ok(format!("crate::{}", names.join("::")))
}

fn find_method(tcx: TyCtxt<'_>, selected: &str) -> Result<LocalDefId> {
    let (owner, name) = selected
        .rsplit_once("::")
        .ok_or("expected qualified method selection")?;
    let mut candidates = tcx.iter_local_def_id().filter(|id| {
        let def = id.to_def_id();
        if !matches!(tcx.def_kind(def), DefKind::AssocFn)
            || !tcx
                .opt_item_name(def)
                .is_some_and(|candidate| candidate.as_str() == name)
        {
            return false;
        }
        let Some(implementation) = tcx.inherent_impl_of_assoc(def) else {
            return false;
        };
        let ty = tcx
            .type_of(implementation)
            .instantiate_identity()
            .skip_normalization();
        matches!(ty.kind(),ty::Adt(adt,_) if path(tcx,adt.did()).as_deref()==Ok(owner))
    });
    let result = candidates
        .next()
        .ok_or("selected inherent method is absent")?;
    if candidates.next().is_some() {
        return Err("selected inherent method is ambiguous".into());
    }
    Ok(result)
}

fn inspect(tcx: TyCtxt<'_>, def: LocalDefId) -> Result<Vec<Value>> {
    let signature = tcx.fn_sig(def).instantiate_identity().skip_binder();
    if signature.safety().is_unsafe()
        || signature.inputs().len() != 2
        || signature.c_variadic()
        || signature.abi().as_str() != "Rust"
        || !signature.output().is_bool()
        || !tcx.associated_item(def).is_method()
        || tcx.generics_of(def).count() != 0
        || !tcx.visibility(def).is_public()
    {
        return Err("expected a public safe non-generic two-reference boolean method".into());
    }
    let inputs: Vec<_> = signature
        .inputs()
        .iter()
        .map(|ty| match ty.kind() {
            ty::Ref(_, target, mutability) if !mutability.is_mut() => match target.kind() {
                ty::Adt(adt, args) if adt.is_struct() && args.is_empty() => path(tcx, adt.did()),
                _ => Err("method inputs must be concrete local structs".into()),
            },
            _ => Err("method inputs must be shared record references".into()),
        })
        .collect::<Result<_>>()?;
    let body = tcx.hir_body_owned_by(def);
    let [receiver, argument] = body.params else {
        return Err("unexpected compiler parameters".into());
    };
    let binding = |param: &rustc_hir::Param<'_>| match param.pat.kind {
        PatKind::Binding(_, id, _, None) => Ok(id),
        _ => Err("unsupported parameter binding or destructuring".to_owned()),
    };
    let locals = [binding(receiver)?, binding(argument)?];
    let typeck = tcx.typeck(def);
    let observer = Observer {
        tcx,
        def,
        typeck,
        locals,
        inputs,
    };
    let ExprKind::Block(block, None) = body.value.kind else {
        return Err("expected a plain method body".into());
    };
    if !block.stmts.is_empty() {
        return Err("statements, aliases or effects in the method body".into());
    }
    let mut result = Vec::new();
    observer.walk(
        block.expr.ok_or("missing boolean tail expression")?,
        &mut result,
        0,
    )?;
    Ok(result)
}

struct Observer<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    def: LocalDefId,
    typeck: &'a TypeckResults<'tcx>,
    locals: [rustc_hir::HirId; 2],
    inputs: Vec<String>,
}

impl<'tcx> Observer<'_, 'tcx> {
    fn walk(&self, expr: &Expr<'tcx>, result: &mut Vec<Value>, depth: usize) -> Result<()> {
        if expr.span.from_expansion() {
            return Err("macro-expanded conjunction expressions are unresolved".into());
        }
        if depth == 256 || result.len() == 256 {
            return Err("field conjunction exceeds recovery depth".into());
        }
        let ExprKind::Binary(op, left, right) = expr.kind else {
            return Err("unsupported expression, alias, or effect in field conjunction".into());
        };
        match op.node {
            BinOpKind::And => {
                self.walk(left, result, depth + 1)?;
                self.walk(right, result, depth + 1)
            }
            BinOpKind::Eq => {
                let (left_field, left_type, left_owner) = self.field(left)?;
                let (right_field, right_type, right_owner) = self.field(right)?;
                if left_owner == right_owner || left_type != right_type {
                    return Err(
                        "equality operands must be fields of distinct inputs with the same type"
                            .into(),
                    );
                }
                let (shape, proof) = self.equality(expr, left_type)?;
                result.push(
                    json!({"left":left_field,"right":right_field,"type":shape,"proof":proof}),
                );
                Ok(())
            }
            _ => Err("only ordered short-circuit conjunctions of equality are supported".into()),
        }
    }

    fn field(&self, expr: &Expr<'tcx>) -> Result<(Value, Ty<'tcx>, usize)> {
        if expr.span.from_expansion() {
            return Err("macro-expanded field operands are unresolved".into());
        }
        let ExprKind::Field(base, spelling) = expr.kind else {
            return Err("equality operand is not a direct field".into());
        };
        let ExprKind::Path(QPath::Resolved(None, native)) = base.kind else {
            return Err("field operand uses indirection or an alias".into());
        };
        let Res::Local(id) = native.res else {
            return Err("field receiver is not a method input binding".into());
        };
        let index = self
            .locals
            .iter()
            .position(|local| *local == id)
            .ok_or("field receiver shadows a method input")?;
        let base_type = self.typeck.expr_ty(base);
        let ty::Ref(_, owner, mutability) = base_type.kind() else {
            return Err("non-reference field owner".into());
        };
        let ty::Adt(adt, args) = owner.kind() else {
            return Err("non-record field owner".into());
        };
        if mutability.is_mut()
            || !adt.is_struct()
            || !args.is_empty()
            || path(self.tcx, adt.did())? != self.inputs[index]
        {
            return Err("field owner does not resolve to the selected record type".into());
        }
        use ty::adjustment::{Adjust, Adjustment, DerefAdjustKind};
        if !matches!(self.typeck.expr_adjustments(base),
            [Adjustment { kind: Adjust::Deref(DerefAdjustKind::Builtin), target }] if target == owner)
        {
            return Err("field access requires a non-builtin receiver adjustment".into());
        }
        let field = adt
            .non_enum_variant()
            .fields
            .get(self.typeck.field_index(expr.hir_id))
            .filter(|field| field.name == spelling.name)
            .ok_or("field identity disagrees with the resolved owner")?;
        Ok((
            json!({"parameter":index,"owner":self.inputs[index],"field":field.name.to_string()}),
            self.typeck.expr_ty(expr),
            index,
        ))
    }

    fn equality(&self, expr: &Expr<'tcx>, value: Ty<'tcx>) -> Result<(Value, &'static str)> {
        let tcx = self.tcx;
        let operator = self.typeck.type_dependent_def_id(expr.hir_id);
        if operator.is_none() {
            return match value.kind() {
                ty::Bool => Ok((json!({"kind":"bool"}), "primitive-equality")),
                ty::Uint(n) => match n.name_str() {
                    "u8" | "u16" | "u32" | "u64" => Ok((
                        json!({"kind":"unsigned","bits":n.name_str()[1..].parse::<u8>().expect("known width")}),
                        "primitive-equality",
                    )),
                    _ => Err("unsupported unsigned equality width".into()),
                },
                _ => Err("unsupported primitive or unresolved equality".into()),
            };
        }
        let operator = operator.unwrap();
        if tcx.item_name(operator).as_str() != "eq"
            || !tcx
                .trait_of_assoc(operator)
                .is_some_and(|owner| Some(owner) == tcx.lang_items().eq_trait())
        {
            return Err("unproven overloaded equality trait".into());
        }
        let arguments = self.typeck.node_args(expr.hir_id);
        let instance = ty::Instance::try_resolve(
            tcx,
            ty::TypingEnv::post_analysis(tcx, self.def),
            operator,
            arguments,
        )
        .map_err(|_| "failed equality instance resolution")?
        .ok_or("unresolved equality instance")?;
        let ty::InstanceKind::Item(callee) = instance.def else {
            return Err("indirect or compiler-shim equality".into());
        };
        let source = tcx.crate_name(callee.krate);
        match value.kind() {
            ty::Array(element,size) if matches!(element.kind(),ty::Uint(ty::UintTy::U8)) && source.as_str()=="core" => {
                let size=size.try_to_target_usize(tcx).filter(|n|*n>0&&u32::try_from(*n).is_ok())
                    .ok_or("unproven fixed byte array length")?;
                Ok((json!({"kind":"fixed-bytes","size":size}),"core-byte-array-equality"))
            }
            ty::Adt(adt,args) if tcx.is_diagnostic_item(Symbol::intern("Vec"),adt.did())
                && tcx.crate_name(adt.did().krate).as_str()=="alloc"
                && source.as_str()=="alloc" && matches!(args.type_at(0).kind(),ty::Uint(ty::UintTy::U8))
                && args.types().nth(1).is_some_and(|ty|ty.is_adt() &&
                    matches!(ty.kind(),ty::Adt(adt,_) if tcx.crate_name(adt.did().krate).as_str()=="alloc"
                        && tcx.item_name(adt.did()).as_str()=="Global")) =>
                Ok((json!({"kind":"bytes"}),"alloc-byte-vector-equality")),
            ty::Adt(adt,args) if args.is_empty() && adt.is_enum() && adt.did().is_local()
                && adt.variants().iter().all(|case|case.fields.is_empty())
                && callee.is_local() && pure_discriminant(tcx,callee) =>
                Ok((json!({"kind":"named","path":path(tcx,adt.did())?}),"local-discriminant-equality")),
            _=>Err("overloaded equality is unresolved or not proven effect-free".into()),
        }
    }
}

fn pure_discriminant(tcx: TyCtxt<'_>, callee: DefId) -> bool {
    let body = tcx.optimized_mir(callee);
    if body.arg_count != 2 || body.basic_blocks.len() != 1 {
        return false;
    }
    let block = &body.basic_blocks[mir::START_BLOCK];
    let assignments = block
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {
                None
            }
            kind => Some(kind),
        })
        .collect::<Vec<_>>();
    let [
        StatementKind::Assign(left),
        StatementKind::Assign(right),
        StatementKind::Assign(compare),
    ] = assignments.as_slice()
    else {
        return false;
    };
    let projection = |stmt: &(mir::Place<'_>, Rvalue<'_>), input: usize| {
        let Rvalue::Discriminant(place) = stmt.1 else {
            return false;
        };
        stmt.0.projection.is_empty()
            && place.local.as_usize() == input
            && matches!(place.projection.as_slice(), [ProjectionElem::Deref])
    };
    let operand_local = |operand: &Operand<'_>| match operand {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
            Some(place.local)
        }
        _ => None,
    };
    let Rvalue::BinaryOp(mir::BinOp::Eq, operands) = &compare.1 else {
        return false;
    };
    projection(left, 1)
        && projection(right, 2)
        && compare.0.local == mir::RETURN_PLACE
        && compare.0.projection.is_empty()
        && left.0.local.as_usize() > 2
        && right.0.local.as_usize() > 2
        && left.0.local != right.0.local
        && operand_local(&operands.0) == Some(left.0.local)
        && operand_local(&operands.1) == Some(right.0.local)
        && matches!(block.terminator().kind, TerminatorKind::Return)
}
