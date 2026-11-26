# Sized tagged binary frames

An `oddlang-binary-frame-v1` graph declares a byte tag, big-endian u32 payload length and a maximum payload size. A payload is one declared tuple or named case. The graph assigns the case tags and ordered slots, with separate named errors for header truncation, oversized declared length, length mismatch and unknown tag.

Slots bind unsigned integers, fixed byte arrays, explicit enum tag tables, terminal bounded bytes, constant tag bytes and checked record fragments. Decoding checks numeric bounds and enum or constant tags when reading the slot. The decoder then applies ordered case guards and checks for unexpected trailing bytes. Encoding applies its own declared guard order before writing a payload. Guards support ranges, data counts, ordered numeric comparisons and checked offset plus byte length. All frame-derived obligations are published with the graph pin.

A binary record with `fragment: true` exposes crate-private checked encode and prefix-read methods for reuse by framed unions. Prefix read consumes only that record's fields. The frame checks the record's native validator before reading later slots. A Rust overlay may bind a fragment to an already declared native struct through a qualified `existingType`; the projection emits layout methods on that type. A standalone record keeps its normal trailing-byte precedence.

The Rust binding names the public frame and incremental decoder. The decoder retains one bounded incomplete frame, consumes at most one frame per call, and returns the consumed count and completed value. Once a read fails, the decoder rejects further input with the named poison error. Finish succeeds only at a clean boundary and does not mutate state. The package component kind is `binary-frame`. [Independent binary frame fixture](../../verification/binary-frame/verify.py)

## Compiler-backed encoder structure

The pinned compiler observer selects one inherent method through `ODD_RECOVERY_TAGGED_FRAME`. It checks one shared enum receiver, a fresh standard byte vector, an exhaustive variant match with distinct literal tags, a dominating payload bound, and the returned tag, big-endian length and payload in that order. The observer tracks each field through compiler-resolved pattern bindings and supported writes. It recovers byte-array extensions, remaining byte vectors, big-endian unsigned values, literal bytes, and complete one-byte enum mappings. It follows bounded local field writers that directly forward the same output buffer.

A case with an unknown write, unsupported encoding, or a possible payload alias keeps its discriminator observation and publishes an unresolved payload obligation. The observer records checked guards and nested record behavior as separate obligations alongside the observed write order. For these rows, recovery requires explicitly approved obligations and corresponding existing portable contracts. The receipt ties the scope to the compiler and source inventory. Decoder behavior, guard semantics and nested record interiors require separate verification.

Run `oddlang recover tagged-frame CRATE_ROOT SOURCE EVIDENCE SELECTION TEMPLATE OVERLAY OUTPUT`. Recovery publishes the graph and a declaration-keyed obligation report together. The selection pins the source and receipt. The template supplies a sorted slot inventory and vacant tag, literal and maximum values. Positional hints bind existing tuple indices to stable slot identities. Rust source and compiler evidence are inputs to recovery. The generated library consumes the portable graph and Rust overlay.

The unrelated native fixture lives at `verification/rust-recovery/tagged-frame/verify.py`. It includes direct and helper-based writes, baseline and mutated native byte comparisons, and rejections for overlapping tags, an altered output header, local buffer aliasing, unproven buffer mutation and unsupported endianness.

## Compiler-backed encoder preflight

The frame guard observer selects the encoder and named cases with `ODD_RECOVERY_TAGGED_FRAME_GUARDS=crate::Packet::encode#CaseA,CaseB`. It reuses the compiler-proven frame layout before reading each case's original preflight order. The supported rules are a nonempty bounded byte vector, an unsigned upper bound, checked offset plus byte count, and an ordered comparison of unsigned fields. It pins the exact resolved error variants and source inventory. A call through `?` must have the encoder's standard Result error type. Unsupported helpers, effectful statements, or guards interleaved with writes produce unresolved evidence.

Run `oddlang recover frame-guards CRATE_ROOT SOURCE EVIDENCE SELECTION TEMPLATE OVERLAY GRAPH` to populate vacant encoder guard lists. The selection pins the source and receipt, and maps each rule's semantic field role to an existing stable identifier. Count and range bounds must agree with the paired slot policy. The decoder's ordered checks remain defined by its own graph contracts. The generated encoder checks preconditions in the observed source order.

The unrelated witness in `verification/rust-recovery/frame-guards/verify.py` exercises simultaneous failures, maximum boundaries, checked overflow and exact valid bytes. It also exercises proof failures for a side effect, altered error conversion and interleaving a check with a write.

## Compiler-backed checked record fragments

Select the encoder and cases with `ODD_RECOVERY_TAGGED_FRAME_FRAGMENTS=crate::Packet::encode#CaseA,CaseB`. The compiler observer supports a checked append into the frame payload and a checked inherent encoder that returns a fresh record payload. It resolves the same record field at the check and the write, verifies standard Result propagation, and records any proved writes that follow the fragment.

Record writers can supply direct fixed bytes, big-endian primitive values, one-byte enum tags, boolean bytes, bounded length-prefixed bytes and optional scalar or inline-record payloads. The observer verifies the optional markers, forwarding of the matched value, and complete ordered inline members. Unsupported operations or altered delegation produce an unresolved result.

Run `oddlang recover frame-fragment CRATE_ROOT SOURCE EVIDENCE SELECTION TEMPLATE OVERLAY OUTPUT`. The selection binds the source proof to an unordered portable catalog of field encodings and reviewed independent record bounds. Recovery writes `graph.json` and `dependencies.json`. The latter records the checked validator, expected validation order, any length bounds required before narrowing, and the outstanding independent qualification of validator and decoder bodies. The recovered writer order must preserve the existing validation order for fields that can reject.

The unrelated native fixture at `verification/rust-recovery/frame-fragments/verify.py` recompiles both delegation forms before and after a source edit. It checks valid bytes and named rejections, including omitted prechecks, changed option markers, incomplete inline writes and unknown buffer mutation.
