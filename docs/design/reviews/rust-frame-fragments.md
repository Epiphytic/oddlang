# Checked frame fragment recovery review

Claude CLI auto review examined the compiler observer and graph recovery. It identified missing trailing frame writes, incomplete optional record checks, possible enum tag truncation and the independent length bounds required before narrowing. It also requested an explicit record of unproved validator and decoder behavior. The implementation now checks complete optional fields and unique in-range tags, pins trailing writes, preserves the reviewed validation order and publishes each remaining independent dependency.

The follow-up found no blockers in those fixes. The unrelated fixture checks both delegation forms, native byte layouts after a source edit, and refused observations for missing prechecks or unsupported writes.

The Mac ARM64 native verifier compiled two upstream and two generated libraries and passed 36 rows. The two source edits changed both recovered write orders and four successful frame outputs. Named version, path and metadata failures matched independent expectations. The companion review requested a more precise decoder evidence reference and explicit partial validation-order coverage. Both were recorded before merge. [Native review and evidence](https://github.com/Epiphytic/marmish-oddlang/pull/59)
