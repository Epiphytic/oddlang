# Checked frame fragment recovery review

Claude CLI auto review examined the compiler observer and graph recovery. It identified missing trailing frame writes, incomplete optional record checks, possible enum tag truncation and the independent length bounds required before narrowing. It also requested an explicit record of unproved validator and decoder behavior. The implementation now checks complete optional fields and unique in-range tags, pins trailing writes, preserves the reviewed validation order and publishes each remaining independent dependency.

The follow-up found no blockers in those fixes. The unrelated fixture checks both delegation forms, native byte layouts after a source edit, and refused observations for missing prechecks or unsupported writes.
