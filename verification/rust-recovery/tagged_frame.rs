//! Compiler observations of an exhaustive enum encoder with a fresh byte buffer.
mod fragments;
mod guards;
pub use fragments::observe_fragments;
pub use guards::observe_guards;

use rustc_ast::LitKind;
use rustc_hir::{
    BinOpKind, Block, BlockCheckMode, Expr, ExprKind, HirId, MatchSource, Pat, PatExprKind,
    PatKind, QPath, Stmt, StmtKind, UnOp,
    def::{DefKind, Res},
};
use rustc_middle::ty::{self, Ty, TyCtxt, TypeckResults};
use rustc_span::{
    Symbol,
    def_id::{DefId, LocalDefId},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

type Result<T> = std::result::Result<T, String>;
type Fields = HashMap<HirId, String>;

pub fn observe(tcx: TyCtxt<'_>, compiler: &str, selected: &str) -> Value {
    let mut evidence = json!({"format":"oddlang-rust-tagged-frame-v1",
        "compiler":compiler, "method":selected, "unresolved":[]});
    let result = super::relations::method_evidence(tcx, selected, &mut evidence)
        .and_then(|def| Observer::new(tcx, def).inspect(def, None, None));
    match result {
        Ok((maximum, cases, _)) => {
            evidence["maximum"] = json!(maximum);
            evidence["cases"] = json!(cases);
        }
        Err(reason) => evidence["unresolved"] = json!([reason]),
    }
    evidence
}

struct Observer<'tcx> {
    tcx: TyCtxt<'tcx>,
    typeck: &'tcx TypeckResults<'tcx>,
}

impl<'tcx> Observer<'tcx> {
    fn new(tcx: TyCtxt<'tcx>, def: LocalDefId) -> Self {
        Self {
            tcx,
            typeck: tcx.typeck(def),
        }
    }

    fn inspect(
        &self,
        def: LocalDefId,
        guards: Option<&BTreeSet<String>>,
        fragments: Option<&BTreeSet<String>>,
    ) -> Result<(u32, Vec<Value>, BTreeMap<String, Value>)> {
        let sig = self.tcx.fn_sig(def).instantiate_identity().skip_binder();
        let [receiver] = sig.inputs() else {
            return Err("encoder requires one receiver".into());
        };
        let ty::Ref(_, inner, mutability) = receiver.kind() else {
            return Err("encoder receiver needs a shared enum reference".into());
        };
        let ty::Adt(adt, args) = inner.kind() else {
            return Err("encoder receiver needs a local enum".into());
        };
        if mutability.is_mut()
            || !adt.is_enum()
            || !adt.did().is_local()
            || !args.is_empty()
            || !self.tcx.associated_item(def).is_method()
            || self.tcx.generics_of(def).count() != 0
            || sig.safety().is_unsafe()
        {
            return Err("unsupported encoder method signature".into());
        }
        let body = self.tcx.hir_body_owned_by(def);
        let [receiver] = body.params else {
            return Err("expected a single receiver declaration".into());
        };
        let (statements, output) = block(body.value)?;
        let [payload, tags, limit, result, push, length, append] = statements else {
            return Err("encoder uses an unfamiliar frame envelope".into());
        };
        let (payload, init) = local_declaration(payload)?;
        if !self.vec_constructor(init, "new", 0) {
            return Err("payload must start as a fresh standard byte vector".into());
        }
        let (tag, choices) = local_declaration(tags)?;
        let ExprKind::Match(scrutinee, arms, MatchSource::Normal) = choices.kind else {
            return Err("tag must come from a direct inherent variant match".into());
        };
        if local(scrutinee) != Some(binding(receiver.pat)?) || arms.len() != adt.variants().len() {
            return Err("frame tag match must exhaust the declared receiver enum".into());
        }

        let maximum = self.maximum(statement(limit)?, payload)?;
        let (result, init) = local_declaration(result)?;
        self.check_envelope(
            payload,
            tag,
            result,
            init,
            statement(push)?,
            statement(length)?,
            statement(append)?,
            output,
        )?;
        let mut seen_variants = BTreeSet::new();
        let mut seen_tags = BTreeSet::new();
        let mut cases = Vec::new();
        let mut recovered_guards = BTreeMap::new();
        for arm in arms {
            if arm.guard.is_some() || arm.pat.span.from_expansion() {
                return Err("guarded or macro-generated variant matches are unresolved".into());
            }
            let (variant, form, fields) = self.variant(*adt, arm.pat)?;
            let (stmts, tail) = block(arm.body)?;
            let tag = integer(tail)
                .and_then(|n| u8::try_from(n).ok())
                .ok_or("arm must return a literal byte discriminator")?;
            if !seen_tags.insert(tag) || !seen_variants.insert(variant.clone()) {
                return Err("frame discriminator or variant collision".into());
            }
            let mut gaps = Vec::new();
            let writes = stmts.iter().try_fold(Vec::new(), |mut writes, stmt| {
                let expr = statement(stmt)?;
                self.arm_write(expr, payload, &fields, &mut writes, &mut gaps)?;
                Ok::<_, String>(writes)
            });
            let shape = match writes {
                Ok(writes) => {
                    json!({"variant":variant, "form":form, "tag":tag, "writes":writes, "unresolved":gaps})
                }
                Err(reason) => {
                    json!({"variant":variant, "form":form, "tag":tag, "unresolved":[reason]})
                }
            };
            if guards.is_some_and(|cases| cases.contains(&variant)) {
                let proof = self
                    .guard_plan(stmts, &fields, &shape, sig.output())
                    .map_err(|why| format!("{variant}: {why}"))?;
                recovered_guards.insert(variant.clone(), proof);
            }
            if fragments.is_some_and(|cases| cases.contains(&variant)) {
                let proof = self
                    .fragment_plan(stmts, payload, &fields, &shape, sig.output())
                    .map_err(|why| format!("{variant}: {why}"))?;
                recovered_guards.insert(variant, proof);
            }
            cases.push(shape);
        }
        if guards
            .or(fragments)
            .is_some_and(|chosen| !chosen.iter().eq(recovered_guards.keys()))
        {
            return Err("selected proof case lacks an observed variant".into());
        }
        Ok((maximum, cases, recovered_guards))
    }

    fn variant(
        &self,
        adt: ty::AdtDef<'tcx>,
        pattern: &Pat<'tcx>,
    ) -> Result<(String, &'static str, Fields)> {
        let (path, form, binding_pats): (_, _, Vec<_>) = match pattern.kind {
            PatKind::Struct(ref path, fields, None) => (
                path,
                "named",
                fields
                    .iter()
                    .map(|field| (field.ident.name.to_string(), field.pat))
                    .collect(),
            ),
            PatKind::TupleStruct(ref path, fields, rest) if rest.as_opt_usize().is_none() => (
                path,
                "tuple",
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| (i.to_string(), field))
                    .collect(),
            ),
            _ => return Err("unsupported variant pattern or missing fields".into()),
        };
        let Res::Def(kind, def) = self.typeck.qpath_res(path, pattern.hir_id) else {
            return Err("unresolved variant definition".into());
        };
        let variant_def = if matches!(kind, DefKind::Ctor(..)) {
            self.tcx.parent(def)
        } else {
            def
        };
        let variant = adt
            .variants()
            .iter()
            .find(|variant| variant.def_id == variant_def)
            .ok_or("match pattern does not resolve to receiver variant")?;
        if variant.fields.len() != binding_pats.len() {
            return Err("match does not bind every variant member".into());
        }
        let fields = binding_pats
            .into_iter()
            .map(|(field, pat)| {
                if !variant.fields.iter().any(|def| def.name.as_str() == field) {
                    return Err("unresolved compiler variant field".into());
                }
                Ok((binding(pat)?, field))
            })
            .collect::<Result<Fields>>()?;
        Ok((variant.name.to_string(), form, fields))
    }

    fn maximum(&self, expr: &Expr<'tcx>, payload: HirId) -> Result<u32> {
        let ExprKind::If(compare, then, None) = expr.kind else {
            return Err("expected a dominating payload length guard".into());
        };
        let ExprKind::Binary(op, len, limit) = unscoped(compare).kind else {
            return Err("unresolved payload length condition".into());
        };
        if op.node != BinOpKind::Gt || !self.len(len, payload) || !self.returned_error(then) {
            return Err("length guard must return an error before the narrowing cast".into());
        }
        let bound = integer(limit)
            .or_else(|| {
                let def = resolved(self.typeck, limit)?;
                if !matches!(self.tcx.def_kind(def), DefKind::Const { .. }) {
                    return None;
                }
                let value = self.tcx.const_eval_poly(def).ok()?;
                Some(value.try_to_scalar_int()?.to_bits_unchecked())
            })
            .ok_or("unresolved frame payload maximum")?;
        u32::try_from(bound)
            .ok()
            .filter(|value| (1..=u32::MAX - 5).contains(value))
            .ok_or("payload maximum does not fit the frame envelope".into())
    }

    fn check_envelope(
        &self,
        payload: HirId,
        tag: HirId,
        output: HirId,
        init: &Expr<'tcx>,
        push: &Expr<'tcx>,
        length: &Expr<'tcx>,
        append: &Expr<'tcx>,
        tail: &Expr<'tcx>,
    ) -> Result<()> {
        if !self.vec_constructor(init, "with_capacity", 1) {
            return Err("frame output needs its own fresh byte vector".into());
        }
        let ExprKind::Call(_, [capacity]) = init.kind else {
            unreachable!()
        };
        let ExprKind::Binary(op, len, five) = capacity.kind else {
            return Err("frame capacity must use the canonical header width".into());
        };
        if op.node != BinOpKind::Add || !self.len(len, payload) || integer(five) != Some(5) {
            return Err("unresolved frame allocation expression".into());
        }
        let tag_expr = self.vec_call(push, output, "push")?;
        if local(tag_expr) != Some(tag) || !self.is_uint(self.typeck.expr_ty(tag_expr), 8) {
            return Err("first emitted byte must be the selected discriminator".into());
        }
        let n = self.vec_call(length, output, "extend")?;
        let cast = self
            .big_endian(n, 32)
            .ok_or("length is not a big-endian u32")?;
        let ExprKind::Cast(len, _) = cast.kind else {
            return Err("unresolved length-prefix value".into());
        };
        if !self.len(len, payload)
            || local(self.vec_call(append, output, "extend")?) != Some(payload)
            || self
                .result_argument(tail, "Ok")
                .is_none_or(|expr| local(expr) != Some(output))
        {
            return Err("output must contain the canonical length and matching payload".into());
        }
        Ok(())
    }

    fn arm_write(
        &self,
        expr: &Expr<'tcx>,
        buffer: HirId,
        fields: &Fields,
        writes: &mut Vec<Value>,
        gaps: &mut Vec<String>,
    ) -> Result<()> {
        if let ExprKind::Assign(to, from, _) = expr.kind
            && local(to) == Some(buffer)
            && writes.is_empty()
            && let Some((field, helper, path)) = self.fragment_result(from, fields)
        {
            writes.push(json!({"kind":"fragment", "field":field, "path":path, "helper":helper}));
            gaps.push(format!("fragment encoder: {helper}"));
            return Ok(());
        }
        if let Ok(arg) = self.vec_call(expr, buffer, "push") {
            writes.push(self.byte(arg, fields)?);
        } else if let Ok(arg) = self.vec_call(expr, buffer, "extend") {
            writes.push(self.extended(arg, fields)?);
        } else if let Some(arg) = self.try_input(expr) {
            if refers_to(arg, buffer) {
                return Err("checked arm call may alias the payload buffer".into());
            }
            gaps.push(format!("checked guard: {}", self.origin(arg)));
        } else if let ExprKind::If(cond, then, None) = expr.kind
            && !refers_to(cond, buffer)
            && self.returned_error(then)
        {
            gaps.push(format!(
                "conditional guard at line {}",
                self.tcx
                    .sess
                    .source_map()
                    .lookup_char_pos(cond.span.lo())
                    .line
            ));
        } else {
            self.writer(expr, buffer, fields, writes, gaps, 0)?;
        }
        Ok(())
    }

    fn field<'a>(&self, expr: &Expr<'tcx>, fields: &'a Fields) -> Option<&'a String> {
        let source = if let ExprKind::Unary(UnOp::Deref, inner) = expr.kind {
            let ty::Ref(_, target, _) = self.typeck.expr_ty(inner).kind() else {
                return None;
            };
            if self.typeck.expr_ty(expr) != *target
                || self.typeck.type_dependent_def_id(expr.hir_id).is_some()
            {
                return None;
            }
            inner
        } else {
            expr
        };
        local(source).and_then(|id| fields.get(&id))
    }

    fn fragment_result(
        &self,
        expr: &Expr<'tcx>,
        fields: &Fields,
    ) -> Option<(String, String, String)> {
        let call = self.try_input(expr)?;
        let ExprKind::MethodCall(_, receiver, [], _) = call.kind else {
            return None;
        };
        let def = self.typeck.type_dependent_def_id(call.hir_id)?;
        let field = self.field(receiver, fields)?;
        let ty::Ref(_, record, _) = self.typeck.expr_ty(receiver).kind() else {
            return None;
        };
        let ty::Adt(adt, _) = record.kind() else {
            return None;
        };
        if !def.is_local()
            || !adt.is_struct()
            || !self.tcx.inherent_impl_of_assoc(def).is_some_and(|imp| {
                self.tcx
                    .type_of(imp)
                    .instantiate_identity()
                    .skip_normalization()
                    == *record
            })
        {
            return None;
        }
        let path = super::relations::path(self.tcx, adt.did()).ok()?;
        Some((field.clone(), self.tcx.def_path_str(def), path))
    }

    fn writer(
        &self,
        expr: &Expr<'tcx>,
        buffer: HirId,
        fields: &Fields,
        writes: &mut Vec<Value>,
        gaps: &mut Vec<String>,
        depth: u8,
    ) -> Result<()> {
        if depth == 12 {
            return Err("payload writer nesting exceeds recovery limit".into());
        }
        let ExprKind::Call(function, [first, rest @ ..]) = expr.kind else {
            return Err("unknown write or alias can change the payload buffer".into());
        };
        if buffer_binding(first) != Some(buffer) {
            return Err("uncertain payload receiver or alias".into());
        }
        let def = resolved(self.typeck, function)
            .and_then(DefId::as_local)
            .ok_or("unresolved or external payload writer")?;
        if self.tcx.def_kind(def) != DefKind::Fn {
            return Err("payload writer is not a direct local function".into());
        }
        let body = self.tcx.hir_body_owned_by(def);
        let [out, params @ ..] = body.params else {
            return Err("missing writer parameters".into());
        };
        if params.len() != rest.len() {
            return Err("payload writer has unmatched arguments".into());
        }
        let mut projected = Fields::new();
        for (arg, param) in rest.iter().zip(params) {
            projected.insert(
                binding(param.pat)?,
                self.field(arg, fields)
                    .ok_or("writer argument is not a matched field")?
                    .clone(),
            );
        }
        let (stmts, tail) = body_block(body.value)?;
        let nested = Self::new(self.tcx, def);
        let out = binding(out.pat)?;
        let mut recovered = Vec::new();
        let observed = stmts
            .iter()
            .map(statement)
            .chain(tail.map(Ok))
            .try_for_each(|stmt| {
                let expr = stmt?;
                if let Ok(arg) = nested.vec_call(expr, out, "push") {
                    recovered.push(nested.byte(arg, &projected)?);
                } else if let Ok(arg) = nested.vec_call(expr, out, "extend") {
                    recovered.push(nested.extended(arg, &projected)?);
                } else {
                    nested.writer(
                        expr,
                        out,
                        &projected,
                        &mut recovered,
                        &mut Vec::new(),
                        depth + 1,
                    )?;
                }
                Ok::<_, String>(())
            });
        if let Err(reason) = observed {
            // A fragment binding deliberately stops at the record boundary.
            let fragment = if let [arg] = rest {
                as_adt(self.typeck.expr_ty(arg)).filter(|adt| adt.is_struct())
            } else {
                None
            };
            let Some(fragment) = fragment.filter(|_| depth == 0) else {
                return Err(reason);
            };
            let fragment_input = binding(body.params[1].pat)?;
            if !nested.append_only(def, 0)? || !refers_to(body.value, fragment_input) {
                return Err("record writer can mutate or escape its output buffer".into());
            }
            let path = super::relations::path(self.tcx, fragment.did())?;
            let field = self
                .field(&rest[0], fields)
                .ok_or("unknown fragment field")?;
            writes.push(
                json!({"kind":"fragment", "field":field, "path":path, "helper":self.tcx.def_path_str(def)}),
            );
            gaps.push(format!("fragment writer: {}", self.tcx.def_path_str(def)));
        } else {
            if writes.len() + recovered.len() > 256 {
                return Err("too many observed payload writes".into());
            }
            writes.extend(recovered);
        }
        Ok(())
    }

    // A reviewed nested record controls its own bytes, but this proof also
    // requires a writer that only appends and never leaks the outer buffer.
    fn append_only(&self, def: LocalDefId, depth: u8) -> Result<bool> {
        if depth == 12 {
            return Ok(false);
        }
        let sig = self.tcx.fn_sig(def).instantiate_identity().skip_binder();
        let Some(first) = sig.inputs().first() else {
            return Ok(false);
        };
        if sig.safety().is_unsafe()
            || !sig.output().is_unit()
            || self.tcx.generics_of(def).count() != 0
            || !matches!(first.kind(), ty::Ref(_, ty, mutability) if mutability.is_mut() && self.byte_vec(*ty))
        {
            return Ok(false);
        }
        let body = self.tcx.hir_body_owned_by(def);
        let recv = binding(body.params.first().ok_or("missing writer buffer")?.pat)?;
        let (stmts, tail) = body_block(body.value)?;
        for stmt in stmts.iter().map(statement).chain(tail.map(Ok)) {
            let expr = stmt?;
            if let Ok(value) = self.vec_call(expr, recv, "push") {
                if refers_to(value, recv) {
                    return Ok(false);
                }
            } else if let Ok(value) = self.vec_call(expr, recv, "extend") {
                if refers_to(value, recv) || !self.standard_bytes(self.typeck.expr_ty(value)) {
                    return Ok(false);
                }
            } else if let ExprKind::Call(function, [out, rest @ ..]) = expr.kind
                && buffer_binding(out) == Some(recv)
                && rest.iter().all(|arg| !refers_to(arg, recv))
                && let Some(nested) = resolved(self.typeck, function).and_then(DefId::as_local)
                && self.tcx.def_kind(nested) == DefKind::Fn
            {
                if !Self::new(self.tcx, nested).append_only(nested, depth + 1)? {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn standard_bytes(&self, value: Ty<'tcx>) -> bool {
        let value = deref_ty(value);
        match value.kind() {
            ty::Array(element, _) | ty::Slice(element) => self.is_uint(*element, 8),
            _ => self.byte_vec(value),
        }
    }

    fn origin(&self, expr: &Expr<'tcx>) -> String {
        let callee = match expr.kind {
            ExprKind::Call(function, _) => resolved(self.typeck, function),
            ExprKind::MethodCall(..) => self.typeck.type_dependent_def_id(expr.hir_id),
            _ => None,
        };
        callee.map_or_else(
            || {
                format!(
                    "expression at line {}",
                    self.tcx
                        .sess
                        .source_map()
                        .lookup_char_pos(expr.span.lo())
                        .line
                )
            },
            |def| self.tcx.def_path_str(def),
        )
    }

    fn byte(&self, expr: &Expr<'tcx>, fields: &Fields) -> Result<Value> {
        if let Some(literal) = integer(expr).and_then(|n| u8::try_from(n).ok()) {
            return Ok(json!({"kind":"literal", "literal":literal}));
        }
        if let ExprKind::Cast(value, _) = expr.kind
            && self.is_uint(self.typeck.expr_ty(expr), 8)
            && let Some(field) = self.field(value, fields)
            && let Some(adt) = as_adt(self.typeck.expr_ty(value)).filter(|adt| adt.is_enum())
            && adt.variants().iter().all(|case| case.fields.is_empty())
        {
            let tags: BTreeMap<_, _> = adt
                .discriminants(self.tcx)
                .map(|(idx, value)| (adt.variant(idx).name.to_string(), value.val as u8))
                .collect();
            return Ok(
                json!({"kind":"enum", "field":field, "path":super::relations::path(self.tcx, adt.did())?, "tags":tags}),
            );
        }
        if let ExprKind::Call(function, [value]) = expr.kind
            && let Some(field) = self.field(value, fields)
            && let Some(adt) = as_adt(self.typeck.expr_ty(value)).filter(|adt| adt.is_enum())
            && let Some(def) = resolved(self.typeck, function)
                .and_then(DefId::as_local)
                .filter(|def| self.tcx.def_kind(*def) == DefKind::Fn)
        {
            let body = self.tcx.hir_body_owned_by(def);
            let [argument] = body.params else {
                return Err("byte mapping helper signature is unresolved".into());
            };
            let (stmts, tail) = block(body.value)?;
            let ExprKind::Match(scrutinee, arms, MatchSource::Normal) = tail.kind else {
                return Err("byte mapping helper must exhaust the input enum".into());
            };
            if !stmts.is_empty()
                || local(scrutinee) != Some(binding(argument.pat)?)
                || arms.len() != adt.variants().len()
            {
                return Err("byte mapping helper is not a pure discriminant match".into());
            }
            let nested = Self::new(self.tcx, def);
            let mut tags = BTreeMap::new();
            for arm in arms {
                let PatKind::Expr(pat) = arm.pat.kind else {
                    return Err("unsupported mapping arm".into());
                };
                let PatExprKind::Path(ref path) = pat.kind else {
                    return Err("unresolved enum mapping case".into());
                };
                let Res::Def(kind, id) = nested.typeck.qpath_res(path, pat.hir_id) else {
                    return Err("unresolved enum mapping identity".into());
                };
                let def = if matches!(kind, DefKind::Ctor(..)) {
                    self.tcx.parent(id)
                } else {
                    id
                };
                let variant = adt
                    .variants()
                    .iter()
                    .find(|case| case.def_id == def && case.fields.is_empty())
                    .ok_or("mapping case is not a unit variant of the encoded enum")?;
                let number = integer(arm.body)
                    .and_then(|n| u8::try_from(n).ok())
                    .filter(|_| arm.guard.is_none())
                    .ok_or("unresolved enum mapping value")?;
                if tags.insert(variant.name.to_string(), number).is_some() {
                    return Err("duplicate enum mapping case".into());
                }
            }
            return Ok(
                json!({"kind":"enum", "field":field, "path":super::relations::path(self.tcx, adt.did())?, "tags":tags}),
            );
        }
        Err("byte write is not a literal or a proven enum mapping".into())
    }

    fn extended(&self, expr: &Expr<'tcx>, fields: &Fields) -> Result<Value> {
        for bits in [16, 32, 64] {
            if let Some(arg) = self.big_endian(expr, bits) {
                let field = self
                    .field(arg, fields)
                    .ok_or("big-endian input is not a matched field")?;
                return Ok(json!({"kind":"unsigned", "field":field, "bits":bits}));
            }
        }
        let field = self
            .field(expr, fields)
            .ok_or("extension is not sourced from a matched field")?;
        let ty = deref_ty(self.typeck.expr_ty(expr));
        match ty.kind() {
            ty::Array(element, size) if self.is_uint(*element, 8) => {
                let size = size
                    .try_to_target_usize(self.tcx)
                    .ok_or("unresolved byte array size")?;
                Ok(json!({"kind":"fixed", "field":field, "size":size}))
            }
            _ if self.byte_vec(ty) => Ok(json!({"kind":"rest", "field":field})),
            _ => Err("unknown byte extension implementation or type".into()),
        }
    }

    fn vec_constructor(&self, expr: &Expr<'tcx>, name: &str, count: usize) -> bool {
        let ExprKind::Call(callee, args) = expr.kind else {
            return false;
        };
        args.len() == count
            && self.byte_vec(self.typeck.expr_ty(expr))
            && resolved(self.typeck, callee).is_some_and(|def| {
                self.tcx.crate_name(def.krate).as_str() == "alloc"
                    && self.tcx.item_name(def).as_str() == name
            })
    }

    fn byte_vec(&self, value: Ty<'tcx>) -> bool {
        matches!(value.kind(), ty::Adt(adt, args) if
            self.tcx.is_diagnostic_item(Symbol::intern("Vec"), adt.did())
                && self.is_uint(args.type_at(0), 8)
                && args.types().nth(1).is_some_and(|ty| matches!(ty.kind(), ty::Adt(adt, _) if
                    self.tcx.crate_name(adt.did().krate).as_str() == "alloc"
                    && self.tcx.item_name(adt.did()).as_str() == "Global")))
    }

    fn is_uint(&self, value: Ty<'tcx>, bits: u8) -> bool {
        matches!(value.kind(), ty::Uint(n) if match bits {
            8=>*n==ty::UintTy::U8, 16=>*n==ty::UintTy::U16, 32=>*n==ty::UintTy::U32,
            64=>*n==ty::UintTy::U64, _=>false,
        })
    }

    fn vec_call(&self, expr: &Expr<'tcx>, buffer: HirId, name: &str) -> Result<&'tcx Expr<'tcx>> {
        let ExprKind::MethodCall(_, base, [arg], _) = expr.kind else {
            return Err("not a direct byte vector operation".into());
        };
        let expected_crate = if name == "extend" { "core" } else { "alloc" };
        if local(base) != Some(buffer)
            || !self.byte_vec(deref_ty(self.typeck.expr_ty(base)))
            || !self.method(expr, expected_crate, name)
        {
            return Err("unresolved vector operation".into());
        }
        Ok(arg)
    }

    fn method(&self, expr: &Expr<'tcx>, krate: &str, name: &str) -> bool {
        self.typeck
            .type_dependent_def_id(expr.hir_id)
            .is_some_and(|def| {
                self.tcx.crate_name(def.krate).as_str() == krate
                    && self.tcx.item_name(def).as_str() == name
                    && (name != "extend"
                        || self
                            .tcx
                            .trait_of_assoc(def)
                            .is_some_and(|owner| self.tcx.item_name(owner).as_str() == "Extend"))
            })
    }

    fn len(&self, expr: &Expr<'tcx>, buffer: HirId) -> bool {
        matches!(expr.kind, ExprKind::MethodCall(_, receiver, [], _)
            if local(receiver) == Some(buffer) && self.method(expr, "alloc", "len"))
    }

    fn big_endian(&self, expr: &Expr<'tcx>, bits: u8) -> Option<&'tcx Expr<'tcx>> {
        let ExprKind::MethodCall(_, receiver, [], _) = expr.kind else {
            return None;
        };
        (self.method(expr, "core", "to_be_bytes")
            && self.is_uint(self.typeck.expr_ty(receiver), bits))
        .then_some(receiver)
    }

    fn result_argument(&self, expr: &Expr<'tcx>, name: &str) -> Option<&'tcx Expr<'tcx>> {
        let ExprKind::Call(callee, [value]) = expr.kind else {
            return None;
        };
        let ctor = resolved(self.typeck, callee)?;
        if !matches!(self.tcx.def_kind(ctor), DefKind::Ctor(..)) {
            return None;
        }
        let variant = self.tcx.parent(ctor);
        (self.tcx.item_name(variant).as_str() == name
            && self
                .tcx
                .is_diagnostic_item(Symbol::intern("Result"), self.tcx.parent(variant)))
        .then_some(value)
    }

    fn returned_error(&self, expr: &'tcx Expr<'tcx>) -> bool {
        let Ok((stmts, tail)) = body_block(expr) else {
            return false;
        };
        let value = match (stmts, tail) {
            ([], Some(tail)) => Some(tail),
            ([stmt], None) => statement(stmt).ok(),
            _ => None,
        };
        matches!(value.map(|value|value.kind), Some(ExprKind::Ret(Some(error)))
            if self.result_argument(error,"Err").is_some_and(|arg|
                matches!(arg.kind, ExprKind::Path(_) | ExprKind::Lit(_))))
    }

    fn try_input(&self, expr: &Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
        let ExprKind::Match(branch, _, MatchSource::TryDesugar(_)) = expr.kind else {
            return None;
        };
        let ExprKind::Call(_, [checked]) = branch.kind else {
            return None;
        };
        Some(checked)
    }
}

fn body_block<'a>(expr: &'a Expr<'a>) -> Result<(&'a [Stmt<'a>], Option<&'a Expr<'a>>)> {
    let ExprKind::Block(
        Block {
            stmts,
            expr,
            rules: BlockCheckMode::DefaultBlock,
            ..
        },
        None,
    ) = expr.kind
    else {
        return Err("expected a plain unlabelled safe block".into());
    };
    Ok((stmts, *expr))
}

fn block<'a>(expr: &'a Expr<'a>) -> Result<(&'a [Stmt<'a>], &'a Expr<'a>)> {
    let (stmts, tail) = body_block(expr)?;
    Ok((stmts, tail.ok_or("missing block result")?))
}

fn binding(pat: &Pat<'_>) -> Result<HirId> {
    match pat.kind {
        PatKind::Binding(_, id, _, None) => Ok(id),
        _ => Err("unsupported binding or destructuring".into()),
    }
}

fn statement<'a>(stmt: &'a Stmt<'a>) -> Result<&'a Expr<'a>> {
    match stmt.kind {
        StmtKind::Semi(expr) | StmtKind::Expr(expr) => Ok(expr),
        _ => Err("local alias or other unproven payload operation".into()),
    }
}

fn local_declaration<'a>(stmt: &Stmt<'a>) -> Result<(HirId, &'a Expr<'a>)> {
    let StmtKind::Let(declaration) = stmt.kind else {
        return Err("expected an encoder binding".into());
    };
    if declaration.els.is_some() {
        return Err("unexpected fallible binding".into());
    }
    Ok((
        binding(declaration.pat)?,
        declaration
            .init
            .ok_or("encoder binding lacks an initializer")?,
    ))
}

fn local(expr: &Expr<'_>) -> Option<HirId> {
    let ExprKind::Path(QPath::Resolved(None, path)) = expr.kind else {
        return None;
    };
    match path.res {
        Res::Local(id) => Some(id),
        _ => None,
    }
}

fn resolved(typeck: &TypeckResults<'_>, expr: &Expr<'_>) -> Option<DefId> {
    let ExprKind::Path(ref path) = expr.kind else {
        return None;
    };
    match typeck.qpath_res(path, expr.hir_id) {
        Res::Def(_, id) => Some(id),
        _ => None,
    }
}

fn buffer_binding(expr: &Expr<'_>) -> Option<HirId> {
    if let ExprKind::AddrOf(_, mutability, inner) = expr.kind
        && mutability.is_mut()
    {
        return local(inner);
    }
    local(expr)
}

fn integer(expr: &Expr<'_>) -> Option<u128> {
    let ExprKind::Lit(lit) = expr.kind else {
        return None;
    };
    if let LitKind::Int(value, _) = lit.node {
        Some(value.get())
    } else {
        None
    }
}

fn unscoped<'a>(expr: &'a Expr<'a>) -> &'a Expr<'a> {
    match expr.kind {
        ExprKind::DropTemps(inner) => inner,
        _ => expr,
    }
}

fn deref_ty(ty: Ty<'_>) -> Ty<'_> {
    if let ty::Ref(_, inner, _) = ty.kind() {
        *inner
    } else {
        ty
    }
}

fn as_adt(ty: Ty<'_>) -> Option<ty::AdtDef<'_>> {
    match deref_ty(ty).kind() {
        ty::Adt(adt, _) => Some(*adt),
        _ => None,
    }
}

fn refers_to(expr: &Expr<'_>, target: HirId) -> bool {
    use rustc_hir::intravisit::{self, Visitor};
    struct Use {
        target: HirId,
        found: bool,
    }
    impl<'hir> Visitor<'hir> for Use {
        fn visit_expr(&mut self, expr: &'hir Expr<'hir>) {
            self.found |=
                local(expr) == Some(self.target) || matches!(expr.kind, ExprKind::Closure(_));
            if !self.found {
                intravisit::walk_expr(self, expr);
            }
        }
    }
    let mut use_of_local = Use {
        target,
        found: false,
    };
    use_of_local.visit_expr(expr);
    use_of_local.found
}
