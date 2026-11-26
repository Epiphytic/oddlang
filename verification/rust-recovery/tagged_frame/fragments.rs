//! Compiler observations of a checked fragment boundary and its record writer.
use super::*;
use rustc_hir::def::{CtorKind, CtorOf};

pub fn observe_fragments(tcx: TyCtxt<'_>, compiler: &str, selector: &str) -> Value {
    let mut receipt =
        json!({"format":"oddlang-rust-frame-fragments-v1","compiler":compiler,"unresolved":[]});
    let checked = (|| {
        let (method, cases) = selector
            .split_once('#')
            .ok_or("expected method and fragment cases")?;
        let choices: BTreeSet<_> = cases.split(',').map(str::to_owned).collect();
        if choices.iter().any(|name| !crate::identifier(name))
            || choices.len() != cases.split(',').count()
        {
            return Err("invalid fragment variant selector".into());
        }
        receipt["method"] = json!(method);
        let def = super::super::relations::method_evidence(tcx, method, &mut receipt)?;
        let (_, _, selected) = Observer::new(tcx, def).inspect(def, None, Some(&choices))?;
        receipt["cases"] = json!(selected);
        Ok::<_, String>(())
    })();
    if let Err(error) = checked {
        receipt["unresolved"] = json!([error]);
    }
    receipt
}

impl<'tcx> Observer<'tcx> {
    pub(super) fn fragment_plan(
        &self,
        statements: &'tcx [Stmt<'tcx>],
        buffer: HirId,
        fields: &Fields,
        structure: &Value,
        outer_result: Ty<'tcx>,
    ) -> Result<Value> {
        let failure = self
            .result_error(outer_result)
            .ok_or("frame must return a standard Result")?;
        let Some(writes) = structure["writes"].as_array() else {
            return Err("unresolved writes prevent fragment recovery".into());
        };
        let [first, rest @ ..] = statements else {
            return Err("missing checked fragment delegation".into());
        };
        let first = statement(first)?;
        let (style, field, validator, writer, delegate) =
            if let ExprKind::Assign(to, from, _) = first.kind {
                if local(to) != Some(buffer) || !rest.is_empty() || writes.len() != 1 {
                    return Err(
                        "record return must be the sole assignment to the frame payload".into(),
                    );
                }
                let call = self
                    .try_input(from)
                    .ok_or("fragment assignment must propagate the encode result")?;
                let (record, encode) = self.checked_method(call, failure)?;
                let field = self
                    .field(record, fields)
                    .ok_or("delegate does not read a matched record field")?;
                let nested = Self::new(self.tcx, encode);
                let (validator, writer) = nested.record_delegate(encode, failure)?;
                (
                    "returned-record",
                    field,
                    validator,
                    writer,
                    Some(self.tcx.def_path_str(encode)),
                )
            } else {
                let check = self
                    .try_input(first)
                    .ok_or("fragment append must follow a checked precondition")?;
                let (record, validator) = self.checked_method(check, failure)?;
                self.unit_validator(validator)?;
                let field = self
                    .field(record, fields)
                    .ok_or("precondition does not check a matched field")?;
                let [append, ..] = rest else {
                    return Err("missing checked append writer".into());
                };
                let writer = self.writer_call(
                    statement(append)?,
                    buffer,
                    local(record).ok_or("unproven record reader")?,
                )?;
                let gaps = structure["unresolved"]
                    .as_array()
                    .ok_or("missing checked fragment evidence")?;
                if gaps.len() != 2 || writes.first().is_none_or(|item| item["kind"] != "fragment") {
                    return Err(
                        "fragment append has other unproven operations before or after validation"
                            .into(),
                    );
                }
                ("checked-append", field, validator, writer, None)
            };
        let compiler = Self::new(self.tcx, writer);
        let (record, slots) = compiler.record_writer(writer)?;
        let check_path = self.tcx.def_path_str(validator);
        let writer_path = self.tcx.def_path_str(writer);
        if writes[0]["field"] != *field || writes[0]["path"].as_str() != Some(&record) {
            return Err("validated record differs from frame payload slot".into());
        }
        let expected = delegate.as_deref().unwrap_or(&writer_path);
        if writes[0]["helper"].as_str() != Some(expected) {
            return Err("resolved boundary differs from original fragment observation".into());
        }
        Ok(
            json!({"form":structure["form"],"field":field, "style":style,"record":record,
            "validator":check_path,"writer":writer_path,"delegate":delegate,"trailing":writes[1..],
            "unproved":["validator-semantics","validation-order","decoder"],"slots":slots}),
        )
    }

    fn checked_method(
        &self,
        call: &'tcx Expr<'tcx>,
        failure: Ty<'tcx>,
    ) -> Result<(&'tcx Expr<'tcx>, LocalDefId)> {
        let ExprKind::MethodCall(_, receiver, [], _) = call.kind else {
            return Err("fragment check must be a direct no-argument inherent method".into());
        };
        let def = self
            .typeck
            .type_dependent_def_id(call.hir_id)
            .and_then(DefId::as_local)
            .ok_or("fragment check has unresolved dispatch")?;
        let implementation = self
            .tcx
            .inherent_impl_of_assoc(def.to_def_id())
            .ok_or("fragment check may dispatch to a trait")?;
        let sig = self.tcx.fn_sig(def).instantiate_identity().skip_binder();
        if sig.safety().is_unsafe()
            || self.tcx.generics_of(def).count() != 0
            || sig.inputs().len() != 1
            || !matches!(sig.inputs()[0].kind(),ty::Ref(_, record, m) if !m.is_mut()
                && *record == self.tcx.type_of(implementation).instantiate_identity().skip_normalization()
                && *record == deref_ty(self.typeck.expr_ty(receiver)))
            || self.result_error(sig.output()) != Some(failure)
        {
            return Err("fragment check changes ownership, error identity or dispatch".into());
        }
        Ok((receiver, def))
    }

    fn record_delegate(
        &self,
        encode: LocalDefId,
        failure: Ty<'tcx>,
    ) -> Result<(LocalDefId, LocalDefId)> {
        let body = self.tcx.hir_body_owned_by(encode);
        let [receiver] = body.params else {
            return Err("unresolved record encoder signature".into());
        };
        let receiver = binding(receiver.pat)?;
        let (stmts, tail) = block(body.value)?;
        let [check, allocation, writer] = stmts else {
            return Err("record delegate needs a checked fresh payload".into());
        };
        let validate = self
            .try_input(statement(check)?)
            .ok_or("record delegate lacks a checked precondition")?;
        let (checked, validator) = self.checked_method(validate, failure)?;
        if local(checked) != Some(receiver) {
            return Err("record delegate checks a different value".into());
        }
        self.unit_validator(validator)?;
        let (bytes, init) = local_declaration(allocation)?;
        if !self.vec_constructor(init, "new", 0)
            || self.result_argument(tail, "Ok").and_then(local) != Some(bytes)
        {
            return Err("record delegate must return its fresh payload".into());
        }
        Ok((
            validator,
            self.writer_call(statement(writer)?, bytes, receiver)?,
        ))
    }

    fn unit_validator(&self, def: LocalDefId) -> Result<()> {
        let output = self
            .tcx
            .fn_sig(def)
            .instantiate_identity()
            .skip_binder()
            .output();
        if matches!(output.kind(),ty::Adt(_,args) if args.type_at(0).is_unit()) {
            return Ok(());
        }
        Err("fragment precondition must return Result<(), Error>".into())
    }

    fn writer_call(
        &self,
        call: &'tcx Expr<'tcx>,
        buffer: HirId,
        value: HirId,
    ) -> Result<LocalDefId> {
        let ExprKind::Call(callee, [out, data]) = call.kind else {
            return Err("fragment requires one direct append writer".into());
        };
        if buffer_binding(out) != Some(buffer) || local(data) != Some(value) {
            return Err("fragment writer does not use the checked value and fresh payload".into());
        }
        direct_function(self.tcx, self.typeck, callee)
    }

    fn record_writer(&self, writer: LocalDefId) -> Result<(String, Vec<Value>)> {
        let sig = self.tcx.fn_sig(writer).instantiate_identity().skip_binder();
        let [_, value] = sig.inputs() else {
            return Err("record writer has unsupported inputs".into());
        };
        let ty::Ref(_, record, borrow) = value.kind() else {
            return Err("record writer must borrow its data".into());
        };
        let ty::Adt(adt, args) = record.kind() else {
            return Err("writer must use a concrete record".into());
        };
        if borrow.is_mut() || !args.is_empty() || !adt.is_struct() {
            return Err("writer record must have concrete shared fields".into());
        }
        let body = self.tcx.hir_body_owned_by(writer);
        let [out, input] = body.params else {
            return Err("missing record writer bindings".into());
        };
        let out = binding(out.pat)?;
        let input = binding(input.pat)?;
        let path = super::super::relations::path(self.tcx, adt.did())?;
        let observed = self.record_body(body.value, out, input)?;
        if observed.len() != adt.non_enum_variant().fields.len() {
            return Err("writer does not cover the declared record fields exactly".into());
        }
        let distinct: BTreeSet<_> = observed
            .iter()
            .filter_map(|slot| slot["field"].as_str())
            .collect();
        if observed.len() != distinct.len() {
            return Err("writer repeats or ambiguously resolves a record field".into());
        }
        Ok((path, observed))
    }

    fn record_body(&self, body: &'tcx Expr<'tcx>, out: HirId, input: HirId) -> Result<Vec<Value>> {
        let (stmts, tail) = body_block(body)?;
        let mut result = Vec::new();
        for expr in stmts.iter().map(statement).chain(tail.map(Ok)) {
            let expr = expr?;
            if let Ok(value) = self.vec_call(expr, out, "extend") {
                self.extended_member(value, input, &mut result)?;
            } else if let Ok(value) = self.vec_call(expr, out, "push") {
                result.push(self.byte_member(value, input)?);
            } else {
                result.push(self.member_helper(expr, out, input)?);
            }
            if result.len() > 256 {
                return Err("record writer exceeds slot limit".into());
            }
        }
        Ok(result)
    }

    fn member(&self, expr: &Expr<'tcx>, input: HirId) -> Result<(String, Ty<'tcx>)> {
        let expr = if let ExprKind::AddrOf(_, m, inner) = expr.kind {
            if m.is_mut() {
                return Err("record field escapes by mutable alias".into());
            }
            inner
        } else {
            expr
        };
        let ExprKind::Field(base, field) = expr.kind else {
            return Err("expected direct record field projection".into());
        };
        if local(base) != Some(input) {
            return Err("field projection uses a different record or alias".into());
        }
        let source = self.typeck.expr_ty(base);
        let record = deref_ty(source);
        let ty::Adt(adt, args) = record.kind() else {
            return Err("unresolved field owner".into());
        };
        if !adt.is_struct() || !args.is_empty() {
            return Err("unresolved concrete field owner".into());
        }
        use ty::adjustment::{Adjust, Adjustment, DerefAdjustKind};
        let direct = match (source.kind(), self.typeck.expr_adjustments(base)) {
            (
                ty::Ref(_, _, m),
                [
                    Adjustment {
                        kind: Adjust::Deref(DerefAdjustKind::Builtin),
                        target,
                    },
                ],
            ) => !m.is_mut() && *target == record,
            (ty::Adt(..), []) => true,
            _ => false,
        };
        if !direct {
            return Err("unproven record field dereference".into());
        }
        let member = adt
            .non_enum_variant()
            .fields
            .get(self.typeck.field_index(expr.hir_id))
            .filter(|member| member.name == field.name)
            .ok_or("unresolved compiler field index")?;
        Ok((member.name.to_string(), self.typeck.expr_ty(expr)))
    }

    fn byte_member(&self, expr: &Expr<'tcx>, input: HirId) -> Result<Value> {
        if let ExprKind::Cast(member, _) = expr.kind
            && let Ok((name, ty)) = self.member(member, input)
            && let Some(adt) = as_adt(ty).filter(|adt| adt.is_enum())
            && adt.variants().iter().all(|case| case.fields.is_empty())
        {
            let path = super::super::relations::path(self.tcx, adt.did())?;
            let tags: BTreeMap<_, _> = adt
                .discriminants(self.tcx)
                .map(|(index, value)| {
                    let tag =
                        u8::try_from(value.val).map_err(|_| "enum tag does not fit one byte")?;
                    Ok::<_, String>((adt.variant(index).name.to_string(), tag))
                })
                .collect::<Result<_>>()?;
            if tags.values().collect::<BTreeSet<_>>().len() != tags.len() {
                return Err("ambiguous enum byte tags".into());
            }
            return Ok(json!({"field":name,"kind":"enum-u8","path":path,"tags":tags}));
        }
        if let ExprKind::Call(function, [member]) = expr.kind
            && let Some(implementation) = resolved(self.typeck, function)
            && self.tcx.crate_name(implementation.krate).as_str() == "core"
            && self.tcx.item_name(implementation).as_str() == "from"
            && let Ok((name, ty)) = self.member(member, input)
            && ty.is_bool()
            && self.is_uint(self.typeck.expr_ty(expr), 8)
        {
            return Ok(json!({"field":name,"kind":"bool"}));
        }
        Err("unproven enum cast or boolean byte conversion".into())
    }

    fn number_member(&self, expr: &Expr<'tcx>, input: HirId) -> Result<Option<Value>> {
        let Some((read, bits, signed)) = self.number_bytes(expr) else {
            return Ok(None);
        };
        let (name, _) = self.member(read, input)?;
        Ok(Some(
            json!({"field":name,"kind":if signed{"signed-be"}else{"unsigned-be"},"bits":bits}),
        ))
    }

    fn number_bytes(&self, expr: &Expr<'tcx>) -> Option<(&'tcx Expr<'tcx>, u8, bool)> {
        let ExprKind::MethodCall(_, value, [], _) = expr.kind else {
            return None;
        };
        if !self.method(expr, "core", "to_be_bytes") {
            return None;
        }
        let (bits, signed) = match self.typeck.expr_ty(value).kind() {
            ty::Uint(ty::UintTy::U16) => (16, false),
            ty::Uint(ty::UintTy::U32) => (32, false),
            ty::Uint(ty::UintTy::U64) => (64, false),
            ty::Int(ty::IntTy::I16) => (16, true),
            ty::Int(ty::IntTy::I32) => (32, true),
            ty::Int(ty::IntTy::I64) => (64, true),
            _ => return None,
        };
        Some((value, bits, signed))
    }

    fn extended_member(&self, expr: &Expr<'tcx>, input: HirId, out: &mut Vec<Value>) -> Result<()> {
        if let Some(numeric) = self.number_member(expr, input)? {
            out.push(numeric);
        } else if let ExprKind::Array(parts) = expr.kind {
            for value in parts {
                out.push(self.byte_member(value, input)?);
            }
        } else {
            let (name, ty) = self.member(expr, input)?;
            let ty::Array(element, size) = ty.kind() else {
                return Err("unproven fixed-byte field".into());
            };
            if !self.is_uint(*element, 8) {
                return Err("fixed field does not contain bytes".into());
            }
            let size = size
                .try_to_target_usize(self.tcx)
                .ok_or("unresolved fixed byte count")?;
            out.push(json!({"field":name,"kind":"fixed-bytes","size":size}));
        }
        Ok(())
    }

    fn member_helper(&self, expr: &Expr<'tcx>, out: HirId, input: HirId) -> Result<Value> {
        let ExprKind::Call(function, [buffer, member, trailing @ ..]) = expr.kind else {
            return Err("unsupported record writer operation".into());
        };
        if buffer_binding(buffer) != Some(out) {
            return Err("record helper uses a different buffer or alias".into());
        }
        let (name, ty) = self.member(member, input)?;
        let def = direct_function(self.tcx, self.typeck, function)?;
        let nested = Self::new(self.tcx, def);
        match trailing {
            [] if self.byte_vec(ty) => {
                nested.length_writer(def)?;
                Ok(json!({"field":name,"kind":"bytes","requiresLengthBound":true}))
            }
            [] => {
                let (bits, signed) = nested.scalar_writer(def)?;
                Ok(
                    json!({"field":name,"kind":if signed{"signed-be"}else{"unsigned-be"},"bits":bits}),
                )
            }
            [callback]
                if as_adt(ty).is_some_and(|adt| {
                    self.tcx
                        .is_diagnostic_item(Symbol::intern("Option"), adt.did())
                }) =>
            {
                nested.option_writer(def)?;
                let payload = self.option_payload(callback)?;
                Ok(json!({"field":name,"kind":"optional","payload":payload}))
            }
            _ => Err("unknown compound record helper".into()),
        }
    }

    fn scalar_writer(&self, def: LocalDefId) -> Result<(u8, bool)> {
        let body = self.tcx.hir_body_owned_by(def);
        let [out, scalar] = body.params else {
            return Err("scalar helper takes unsupported arguments".into());
        };
        let stmt = single_statement(body.value)?;
        let value = self.vec_call(stmt, binding(out.pat)?, "extend")?;
        let (source, bits, signed) = self
            .number_bytes(value)
            .ok_or("scalar helper needs big-endian bytes")?;
        if local(source) != Some(binding(scalar.pat)?) {
            return Err("scalar helper encodes a different value".into());
        }
        Ok((bits, signed))
    }

    fn length_writer(&self, def: LocalDefId) -> Result<()> {
        let body = self.tcx.hir_body_owned_by(def);
        let [out, bytes] = body.params else {
            return Err("length helper takes unsupported arguments".into());
        };
        let out = binding(out.pat)?;
        let bytes = binding(bytes.pat)?;
        let (stmts, tail) = body_block(body.value)?;
        let ([prefix, append], None) = (stmts, tail) else {
            return Err("length helper must write one size and its bytes".into());
        };
        let ExprKind::Call(function, [buffer, size]) = statement(prefix)?.kind else {
            return Err("unresolved length write".into());
        };
        let ExprKind::Cast(length, _) = size.kind else {
            return Err("length count must fit u32".into());
        };
        let ExprKind::MethodCall(_, receiver, [], _) = length.kind else {
            return Err("unresolved length source".into());
        };
        let ty::Slice(element) = deref_ty(self.typeck.expr_ty(receiver)).kind() else {
            return Err("length helper needs a byte slice".into());
        };
        let size_writer = direct_function(self.tcx, self.typeck, function)?;
        if buffer_binding(buffer) != Some(out)
            || local(receiver) != Some(bytes)
            || !self.is_uint(*element, 8)
            || !self.method(length, "core", "len")
            || !self.is_uint(self.typeck.expr_ty(size), 32)
            || Self::new(self.tcx, size_writer).scalar_writer(size_writer)? != (32, false)
            || local(self.vec_call(statement(append)?, out, "extend")?) != Some(bytes)
        {
            return Err("length helper write order or identity unresolved".into());
        }
        Ok(())
    }

    fn option_writer(&self, def: LocalDefId) -> Result<()> {
        let body = self.tcx.hir_body_owned_by(def);
        let [out, option, callback] = body.params else {
            return Err("unexpected optional writer inputs".into());
        };
        let out = binding(out.pat)?;
        let option_stmt = single_statement(body.value)?;
        let ExprKind::Match(value, [a, b], MatchSource::Normal) = option_stmt.kind else {
            return Err("optional writer needs exactly two checked arms".into());
        };
        if local(value) != Some(binding(option.pat)?) {
            return Err("optional writer discriminates a different value".into());
        }
        let Some(adt) = as_adt(self.typeck.expr_ty(value)).filter(|adt| {
            self.tcx
                .is_diagnostic_item(Symbol::intern("Option"), adt.did())
        }) else {
            return Err("unproven optional type".into());
        };
        let mut tags = BTreeSet::new();
        for arm in [a, b] {
            if arm.guard.is_some() {
                return Err("guarded optional arm".into());
            }
            match arm.pat.kind {
                PatKind::TupleStruct(ref path, [item], rest) if rest.as_opt_usize().is_none() => {
                    let Some(def) = variant(self.tcx, self.typeck, path, arm.pat.hir_id) else {
                        return Err("unresolved Some constructor".into());
                    };
                    if self.tcx.parent(def) != adt.did()
                        || self.tcx.item_name(def).as_str() != "Some"
                    {
                        return Err("unresolved Some arm".into());
                    }
                    let (stmts, tail) = body_block(arm.body)?;
                    let [tag, write] = stmts else {
                        return Err("Some must emit its tag then one value".into());
                    };
                    let ExprKind::Call(function, [buffer, argument]) = statement(write)?.kind
                    else {
                        return Err("unknown optional callback".into());
                    };
                    if tail.is_some()
                        || integer(self.vec_call(statement(tag)?, out, "push")?) != Some(1)
                        || local(function) != Some(binding(callback.pat)?)
                        || buffer_binding(buffer) != Some(out)
                        || local(argument) != Some(binding(item)?)
                        || !tags.insert(1)
                    {
                        return Err(
                            "Some must emit marker one and forward its value exactly once".into(),
                        );
                    }
                }
                PatKind::Expr(pat) if let PatExprKind::Path(ref path) = pat.kind => {
                    let Some(def) = variant(self.tcx, self.typeck, path, pat.hir_id) else {
                        return Err("unresolved None constructor".into());
                    };
                    if self.tcx.parent(def) != adt.did()
                        || self.tcx.item_name(def).as_str() != "None"
                        || integer(self.vec_call(arm.body, out, "push")?) != Some(0)
                        || !tags.insert(0)
                    {
                        return Err("None must emit marker zero".into());
                    }
                }
                _ => return Err("unsupported optional match arm".into()),
            }
        }
        if tags.len() != 2 {
            return Err("optional tag mapping is incomplete".into());
        }
        Ok(())
    }

    fn option_payload(&self, expr: &'tcx Expr<'tcx>) -> Result<Value> {
        if let ExprKind::Closure(closure) = expr.kind {
            let body = self.tcx.hir_body(closure.body);
            let [out, value] = body.params else {
                return Err("optional callback must take output and data".into());
            };
            let value_type = self.typeck.node_type(value.pat.hir_id);
            let ty::Adt(adt, args) = value_type.kind() else {
                return Err("optional callback requires an inline record".into());
            };
            if !args.is_empty() || !adt.is_struct() {
                return Err("optional callback must encode a concrete record".into());
            }
            let path = super::super::relations::path(self.tcx, adt.did())?;
            let members = self.record_body(body.value, binding(out.pat)?, binding(value.pat)?)?;
            let distinct: BTreeSet<_> = members
                .iter()
                .filter_map(|member| member["field"].as_str())
                .collect();
            if members.len() != adt.non_enum_variant().fields.len()
                || members.len() != distinct.len()
            {
                return Err("optional callback must write each field exactly once".into());
            }
            return Ok(json!({"kind":"record","path":path,"members":members}));
        }
        let def = direct_function(self.tcx, self.typeck, expr)?;
        let (bits, signed) = Self::new(self.tcx, def).scalar_writer(def)?;
        if signed {
            return Err("unsupported optional signed payload".into());
        }
        Ok(json!({"kind":"unsigned-be","bits":bits}))
    }
}

fn single_statement<'tcx>(expr: &'tcx Expr<'tcx>) -> Result<&'tcx Expr<'tcx>> {
    match body_block(expr)? {
        ([stmt], None) => statement(stmt),
        ([], Some(tail)) => Ok(tail),
        _ => Err("helper must have one checked expression".into()),
    }
}

fn direct_function(
    tcx: TyCtxt<'_>,
    typeck: &TypeckResults<'_>,
    expr: &Expr<'_>,
) -> Result<LocalDefId> {
    resolved(typeck, expr)
        .and_then(DefId::as_local)
        .filter(|id| {
            tcx.def_kind(*id) == DefKind::Fn && {
                let sig = tcx.fn_sig(*id).instantiate_identity().skip_binder();
                !sig.safety().is_unsafe() && sig.output().is_unit()
            }
        })
        .ok_or_else(|| "unresolved or unsafe record helper".into())
}

fn variant(
    tcx: TyCtxt<'_>,
    typeck: &TypeckResults<'_>,
    path: &QPath<'_>,
    id: HirId,
) -> Option<DefId> {
    match typeck.qpath_res(path, id) {
        Res::Def(DefKind::Ctor(CtorOf::Variant, CtorKind::Fn | CtorKind::Const), def) => {
            Some(tcx.parent(def))
        }
        Res::Def(DefKind::Variant, def) => Some(def),
        _ => None,
    }
}
