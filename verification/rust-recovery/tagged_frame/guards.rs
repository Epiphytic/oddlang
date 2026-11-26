//! Compiler-resolved bounded encode preflight rules.
use super::*;

pub fn observe_guards(tcx: TyCtxt<'_>, compiler: &str, selector: &str) -> Value {
    let mut output =
        json!({"format":"oddlang-rust-frame-guards-v1", "compiler":compiler, "unresolved":[]});
    let observed = (|| {
        let (method, variants) = selector
            .split_once('#')
            .ok_or("expected a method and guard cases")?;
        let ordered: Vec<_> = variants.split(',').map(str::to_owned).collect();
        let chosen: BTreeSet<_> = ordered.iter().cloned().collect();
        if chosen.len() != ordered.len() || chosen.iter().any(|name| !crate::identifier(name)) {
            return Err("unknown or repeated guard variant selector".into());
        }
        output["method"] = json!(method);
        let def = super::super::relations::method_evidence(tcx, method, &mut output)?;
        let (_, _, plans) = Observer::new(tcx, def).inspect(def, Some(&chosen))?;
        output["cases"] = json!(plans);
        Ok::<_, String>(())
    })();
    if let Err(reason) = observed {
        output["unresolved"] = json!([reason]);
    }
    output
}

impl<'tcx> Observer<'tcx> {
    pub(super) fn guard_plan(
        &self,
        statements: &'tcx [Stmt<'tcx>],
        fields: &Fields,
        structure: &Value,
        result: Ty<'tcx>,
    ) -> Result<Value> {
        let failure = self
            .result_error(result)
            .ok_or("encoder must return a standard Result")?;
        let Some(gaps) = structure["unresolved"].as_array() else {
            return Err("no compiler proof of ordered frame payload writes".into());
        };
        if structure["writes"].is_null()
            || gaps.iter().any(|gap| {
                gap.as_str().is_none_or(|gap| {
                    !gap.starts_with("checked guard: ")
                        && !gap.starts_with("conditional guard at line ")
                })
            })
        {
            return Err("unknown payload effects prevent guard proof".into());
        }
        let mut guards = Vec::new();
        let mut writing = false;
        for stmt in statements {
            let expr = statement(stmt)?;
            let guard = match expr.kind {
                ExprKind::If(condition, failed, None) => {
                    Some(self.inline_guard(unscoped(condition), failed, fields)?)
                }
                _ => self
                    .try_input(expr)
                    .map(|call| self.helper_guard(call, fields, failure))
                    .transpose()?,
            };
            if let Some(guard) = guard {
                if writing {
                    return Err("guards cannot interleave with payload writes".into());
                }
                guards.push(guard);
            } else {
                writing = true;
            }
        }
        if guards.is_empty() || guards.len() != gaps.len() || !writing {
            return Err("compiler guard plan does not cover the selected preflight".into());
        }
        Ok(json!({"form":structure["form"], "guards":guards}))
    }

    fn inline_guard(
        &self,
        condition: &'tcx Expr<'tcx>,
        failed: &'tcx Expr<'tcx>,
        fields: &Fields,
    ) -> Result<Value> {
        let error = self
            .returned_fault(failed)
            .ok_or("unproven inline guard error constructor")?;
        let ExprKind::Binary(op, left, right) = condition.kind else {
            return Err("unknown inline guard relation".into());
        };
        match op.node {
            BinOpKind::Or => {
                let empty = self
                    .vec_measure(left, "is_empty")
                    .ok_or("unresolved byte-count nonempty check")?;
                let ExprKind::Binary(op, len, upper) = right.kind else {
                    return Err("count limit must use a strict upper comparison".into());
                };
                let sized = self
                    .vec_measure(len, "len")
                    .ok_or("unresolved byte-count length")?;
                let maximum = self
                    .unsigned_constant(upper)
                    .and_then(|v| u32::try_from(v).ok())
                    .filter(|v| *v >= 1)
                    .ok_or("invalid checked byte count")?;
                let bytes = self
                    .field(empty, fields)
                    .ok_or("byte count guard uses an unproven field")?;
                if op.node != BinOpKind::Gt || self.field(sized, fields) != Some(bytes) {
                    return Err(
                        "count guard compares different byte fields or unsupported bounds".into(),
                    );
                }
                Ok(
                    json!({"kind":"count","field":bytes,"minimum":1,"maximum":maximum,"error":error}),
                )
            }
            BinOpKind::Gt if self.known_unsigned_order(condition, left, right) => {
                let a = self
                    .field(left, fields)
                    .ok_or("unproven left field in guard comparison")?;
                let b = self
                    .field(right, fields)
                    .ok_or("unproven right field in guard comparison")?;
                Ok(json!({"kind":"at-most","left":a,"right":b,"error":error}))
            }
            _ => Err("unknown inline guard relation".into()),
        }
    }

    fn helper_guard(
        &self,
        call: &'tcx Expr<'tcx>,
        fields: &Fields,
        failure: Ty<'tcx>,
    ) -> Result<Value> {
        let ExprKind::Call(function, args) = call.kind else {
            return Err("guard must use a direct local checked function".into());
        };
        let def = resolved(self.typeck, function)
            .and_then(DefId::as_local)
            .filter(|id| self.tcx.def_kind(*id) == DefKind::Fn)
            .ok_or("unsupported guard callee or dispatch")?;
        let sig = self.tcx.fn_sig(def).instantiate_identity().skip_binder();
        if self.result_error(sig.output()) != Some(failure) {
            return Err("guard error conversion through ? is unresolved".into());
        }
        if sig.safety().is_unsafe() || self.tcx.generics_of(def).count() != 0 {
            return Err("guard helper requires a safe monomorphic definition".into());
        }
        let body = self.tcx.hir_body_owned_by(def);
        let (stmts, tail) = block(body.value)?;
        if !stmts.is_empty() {
            return Err("unproven statements in guard helper".into());
        }
        let nested = Self::new(self.tcx, def);
        match (args, body.params, sig.inputs()) {
            ([value], [arg], [ty]) if self.is_uint(*ty, 64) => {
                let field = self
                    .field(value, fields)
                    .ok_or("range guard must read a matched offset field")?;
                nested.range_guard(tail, binding(arg.pat)?, field)
            }
            ([offset, size], [start, bytes], [number, len])
                if self.is_uint(*number, 64)
                    && matches!(len.kind(), ty::Uint(ty::UintTy::Usize)) =>
            {
                let offset = self
                    .field(offset, fields)
                    .ok_or("end guard has an unknown offset field")?;
                let size = self
                    .vec_measure(size, "len")
                    .ok_or("end guard count must use byte-vector length")?;
                let field = self
                    .field(size, fields)
                    .ok_or("end guard length must use a matched field")?;
                nested.end_guard(
                    tail,
                    binding(start.pat)?,
                    binding(bytes.pat)?,
                    offset,
                    field,
                )
            }
            _ => Err("unsupported checked helper signature".into()),
        }
    }

    fn range_guard(&self, tail: &'tcx Expr<'tcx>, arg: HirId, field: &str) -> Result<Value> {
        let ExprKind::If(condition, when_bad, Some(when_good)) = tail.kind else {
            return Err("range helper must have exactly one rejection branch".into());
        };
        let ExprKind::Binary(op, left, right) = unscoped(condition).kind else {
            return Err("range helper condition is not an unsigned comparison".into());
        };
        let (bad_stmts, bad) = block(when_bad)?;
        let (good_stmts, good) = block(when_good)?;
        let error = self
            .result_argument(bad, "Err")
            .and_then(|e| self.fault(e))
            .ok_or("unknown range error")?;
        if !bad_stmts.is_empty()
            || !good_stmts.is_empty()
            || self
                .result_argument(good, "Ok")
                .is_none_or(|value| !matches!(value.kind, ExprKind::Tup([])))
            || op.node != BinOpKind::Gt
            || local(left) != Some(arg)
        {
            return Err("range helper does not implement the exact checked bound".into());
        }
        let maximum = self
            .unsigned_constant(right)
            .ok_or("unknown range maximum")?;
        Ok(json!({"kind":"range","field":field,"maximum":maximum,"error":error}))
    }

    fn end_guard(
        &self,
        tail: &'tcx Expr<'tcx>,
        offset_param: HirId,
        count_param: HirId,
        offset: &str,
        bytes: &str,
    ) -> Result<Value> {
        let (filtered, fault) = self
            .option_method(tail, "ok_or")
            .ok_or("end helper must map Option to its declared error")?;
        let error = self.fault(fault).ok_or("unresolved end guard error")?;
        let (added, filter) = self
            .option_method(filtered, "filter")
            .ok_or("end helper lacks a proven limit filter")?;
        let ExprKind::MethodCall(_, offset_expr, [count], _) = added.kind else {
            return Err("end helper does not perform checked addition".into());
        };
        let ExprKind::Cast(source, _) = count.kind else {
            return Err("end helper must widen the byte count".into());
        };
        if !self.method(added, "core", "checked_add")
            || !self.is_uint(self.typeck.expr_ty(offset_expr), 64)
            || !self.is_uint(self.typeck.expr_ty(count), 64)
            || local(offset_expr) != Some(offset_param)
            || local(source) != Some(count_param)
            || self.tcx.sess.target.pointer_width > 64
        {
            return Err("end helper does not perform a lossless checked u64 addition".into());
        }
        let ExprKind::Closure(closure) = filter.kind else {
            return Err("end bound requires a direct predicate closure".into());
        };
        let body = self.tcx.hir_body(closure.body);
        let [param] = body.params else {
            return Err("unproven end predicate input".into());
        };
        let ExprKind::Binary(op, left, right) = body.value.kind else {
            return Err("end predicate is not a direct bound".into());
        };
        let ExprKind::Unary(UnOp::Deref, value) = left.kind else {
            return Err("end predicate must read the checked sum".into());
        };
        if op.node != BinOpKind::Le
            || local(value) != Some(binding(param.pat)?)
            || !self.is_uint(self.typeck.expr_ty(left), 64)
        {
            return Err("end predicate does not check the sum against its bound".into());
        }
        let max = self
            .unsigned_constant(right)
            .ok_or("unknown end guard maximum")?;
        Ok(json!({"kind":"end-at-most","offset":offset,"bytes":bytes,"maximum":max,"error":error}))
    }

    fn result_error(&self, result: Ty<'tcx>) -> Option<Ty<'tcx>> {
        let ty::Adt(adt, args) = result.kind() else {
            return None;
        };
        self.tcx
            .is_diagnostic_item(Symbol::intern("Result"), adt.did())
            .then(|| args.type_at(1))
    }

    fn known_unsigned_order(
        &self,
        compare: &Expr<'tcx>,
        left: &Expr<'tcx>,
        right: &Expr<'tcx>,
    ) -> bool {
        let left = self.typeck.expr_ty(left);
        let right = self.typeck.expr_ty(right);
        if self.is_uint(left, 64) && self.is_uint(right, 64) {
            return true;
        }
        let shared_u64 = |value: Ty<'tcx>| {
            matches!(value.kind(), ty::Ref(_, inner, mutability)
                if !mutability.is_mut() && self.is_uint(*inner, 64))
        };
        // The standard shared-reference implementation delegates to the
        // primitive's ordering. Local code cannot implement this foreign pair.
        shared_u64(left)
            && shared_u64(right)
            && self
                .typeck
                .type_dependent_def_id(compare.hir_id)
                .is_some_and(|def| {
                    self.tcx.item_name(def).as_str() == "gt"
                        && self.tcx.trait_of_assoc(def) == self.tcx.lang_items().partial_ord_trait()
                })
    }

    fn vec_measure(&self, expr: &Expr<'tcx>, name: &str) -> Option<&'tcx Expr<'tcx>> {
        let ExprKind::MethodCall(_, receiver, [], _) = expr.kind else {
            return None;
        };
        (self.method(expr, "alloc", name) && self.byte_vec(deref_ty(self.typeck.expr_ty(receiver))))
            .then_some(receiver)
    }

    fn option_method(
        &self,
        expr: &Expr<'tcx>,
        name: &str,
    ) -> Option<(&'tcx Expr<'tcx>, &'tcx Expr<'tcx>)> {
        let ExprKind::MethodCall(_, receiver, [arg], _) = expr.kind else {
            return None;
        };
        let adt = as_adt(self.typeck.expr_ty(receiver))?;
        (self.method(expr, "core", name)
            && self
                .tcx
                .is_diagnostic_item(Symbol::intern("Option"), adt.did()))
        .then_some((receiver, arg))
    }

    fn returned_fault(&self, expr: &'tcx Expr<'tcx>) -> Option<Value> {
        let (stmts, tail) = body_block(expr).ok()?;
        let value = match (stmts, tail) {
            ([], Some(value)) => value,
            ([stmt], None) => statement(stmt).ok()?,
            _ => return None,
        };
        let ExprKind::Ret(Some(fault)) = value.kind else {
            return None;
        };
        self.fault(self.result_argument(fault, "Err")?)
    }

    fn fault(&self, expr: &Expr<'tcx>) -> Option<Value> {
        let ctor = resolved(self.typeck, expr)?;
        let DefKind::Ctor(rustc_hir::def::CtorOf::Variant, rustc_hir::def::CtorKind::Const) =
            self.tcx.def_kind(ctor)
        else {
            return None;
        };
        let variant = self.tcx.parent(ctor);
        let owner = super::super::relations::path(self.tcx, self.tcx.parent(variant)).ok()?;
        Some(json!({"owner":owner,"variant":self.tcx.item_name(variant).to_string()}))
    }

    fn unsigned_constant(&self, expr: &Expr<'tcx>) -> Option<u64> {
        if !matches!(self.typeck.expr_ty(expr).kind(), ty::Uint(_)) {
            return None;
        }
        if let Some(value) = integer(expr) {
            return u64::try_from(value).ok();
        }
        let def = resolved(self.typeck, expr)?;
        if !matches!(self.tcx.def_kind(def), DefKind::Const { .. }) {
            return None;
        }
        let value = self
            .tcx
            .const_eval_poly(def)
            .ok()?
            .try_to_scalar_int()?
            .to_bits_unchecked();
        u64::try_from(value).ok()
    }
}
