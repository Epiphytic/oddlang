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

## Phase 2. Rust compiler structure

Claude independently ran the compiler fixture and discovered invisible
function-valued operands, missing implicit-drop and nested-projection
gaps, inherited Cargo cfg drift, incomplete input pins and permissive
relationship shapes. Follow-up changes observe references such as
`map(a)` versus `map(b)`, add per-owner gaps and test flag scrubbing.
The pinned run now checks source revision and binds member and declared
adapter inputs. The typed loader checks allowed relationship endpoints
where both endpoint kinds are known. The follow-up reproduced a graph
change caused by parent Cargo configuration. The lock now records those
file digests. A control which adds a cfg-gated declaration changed both
the graph and the recorded ancestry configuration pin.

Compiler ordinal identities can change when an implementation or closure
is inserted. Implicit and indirect effects, external package identity,
several type shapes, visibility distinctions, path dependencies outside the source tree and the independently unreconciled rows keep full
structure qualification open. [Structural evidence](portable-rust-structure.md)

## Phase 3. Tagged processing rules

Claude compared the pinned Data evaluator with upstream on approximately
9.2 million sampled encode, decode and incremental observations. It
reported zero mismatches within that role. Its counterexamples triggered
fail-closed checks for shadowing encoder patterns, decoder input rebinding,
alternate explicit variant construction and overlays that disagree with the
observed field names, widths or order. Follow-up reproduced the source-to-RDF
pin and mutation and verified the first three primary fixes. Regression tests
cover the additional alternate construction finding. Dynamic semantics and
full processing extraction remain open.

## Phase 4. Native targets

Claude independently compiled the Rust module and ran the TypeScript module.
It matched Rust and TypeScript on 7,500 generated boundary and incremental
observations. The follow-up added a generated Rust compile and replay test,
stream-state regressions, noncanonical integer checks and links to the
integrated qualification evidence. TypeScript 5.9.3 also checked the generated
module in strict mode. The native module API uses the scoped portable
signature. Full original Rust bindings and alternative library substitution
remain open. The output directory must be fresh.

## Phase 5. Scoped qualification

Claude reviewed the oracle filters, pin, actual output hashes, six peer
directions and isolation checks. It requested controls for a positive
mutant one-shot and incremental read, wrong encode tags, a named real
structural graph probe, and the distinction between an inaccessible hint
sidecar and choosing an alternative library. The refreshed receipt includes
those controls and additional offset, payload and length boundary cases.
Peer replay compares exact canonical bytes and decoded values. The private
capsule remains outside the checkout and outside the builders.
[Qualification evidence](portable-rust-slice.md)
