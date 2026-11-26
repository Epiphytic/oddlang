# Portable Rust implementation reviews

Claude Code CLI 2.1.332 ran in safe mode with permission mode `auto` in
disposable checkouts. Each review identifies the implementation scope under
test. The matrix and work graph track remaining implementation obligations.

## Phase 1. Boundary and original oracle

Claude reproduced all 403 public stream cases, 31 peer replays and three
mutation failures in an independent replay. It checked the case commitments
and verified the separation of sealed observations from the driver.

The bridge replay initially failed in Claude's environment. An inherited
open-file limit of 1,048,576 caused the original worker validation to report
`worker_unavailable`. The bridge driver now gives the child an open-file
limit of 256 and records the inherited limits. A replay from a parent
with a 1,048,576 open-file limit passed all five cases. The bridge status
and channel controls now mutate the expected fixture values.

The stream documentation identifies the precedence expectations observed
from upstream, the same-domain seeded holdout, untested nested variants, the
development build profile and build-path-dependent executable digests.
The earlier bridge and stream build records identify their linkers and
target configurations. A fully isolated Cargo configuration receipt remains
part of broader profile qualification. [Boundary](portable-rust-boundary.md)
· [Stream oracle](../../examples/marmish/portable-oracle/README.md)
