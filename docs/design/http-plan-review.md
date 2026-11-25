# HTTP implementation plan review

Claude Sonnet reviewed the plan and design on the CLI in auto permission mode on 2026-11-25. The review inspected the local documents and asked whether each phase had the interfaces and tests needed by the next phase.

| Finding | Revision |
| --- | --- |
| Required bindings and target replay lacked file formats and an implementation owner. | Phase 3 defines the overlay and tests required bindings and advisory hints. Phase 5 defines the target lock, replay inputs and tamper check. |
| Implementations could report success without proving which required cases ran. | Phase 2 defines native claims, evidence, selection and verification. The verifier expands a claim from the verified bundle and checks each required case, its outcome and its observations. |
| Stable identifiers and rule versions were underspecified. | Phases 1 and 2 define operation IRIs, the obligation inventory and rule-set versioning. Verified evidence adds the release manifest digest to the key. |
| The HTTP profile left request error order open and the evolution fixture mixed two changes. | The design pins HTTP-only error IRIs and precedence. Phase 5 isolates the rename from the richer type fixtures and deliberately exercises version skew through response decoding. |
| Publishing and adopting a release used an ambiguous lock input. | Phases 1 and 2 give source checking, publishing and release checking distinct commands and inputs. Publication emits a separate workspace pin that a later invocation verifies. |

Claude Fable independently reviewed the initial plan on the CLI in auto permission mode and passed the existing four offline contract release tests before issuing findings. It confirmed the publication and evidence issues and identified these additional gaps.

| Finding | Revision |
| --- | --- |
| Deleting a required witness had no testable seam. The scenario list was incomplete and the test oracle cannot run in the publisher. | Phase 2 exposes coverage checking over inventory and cases. Portable authored expectations cover all six guards, precedence and boundary state, and the tests cross-check them with the existing oracle. |
| V1 had no predecessor and changing the vocabulary could change its pinned bytes. | Publish compatibility only with a predecessor. Preserve rule-set replay and verify republication against the saved v1 pin. Equivalent RDF must yield identical derived artifacts. Exact source bytes are pinned separately. |
| Fetch rewrites dot segments, native JSON decoders accept malformed inputs differently, and fixture startup was implicit. | The design fixes path and JSON semantics. Phases 2–4 define raw body encoding, role-specific peers, provider readiness and tests for parser edge cases and both client codec directions. |
| A mixed-version transfer can commit before decoding fails. Pin-only upgrades require stable native names. | Phase 5 tests the committed state under skew and keeps the field rename in codecs. Phase 6 inspects actual wire output and keeps parallel v1 and v2 deployments. |
| The richer vocabulary and wire migration are independently reviewable changes. | Phase 5 reviews the wire upgrade first and the codec fixture separately. The setup and hybrid steps own runtime and package pinning. |

A Sonnet follow-up confirmed the revised lock, obligation and evolution interfaces and identified three remaining handoffs. The final plan locates authored cases in a pinned source file, gives a field its wire and native names in Phase 1, and gives TypeScript an explicit overlay. The standalone Go adapter can be locked without an overlay.
