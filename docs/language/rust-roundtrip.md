# Native Rust syntax bundles

A Rust syntax bundle is the source of truth for regenerating a Cargo package's
Rust files. Cargo still resolves dependencies, expands macros, runs build
scripts and compiles the result. Cargo manifests, the dependency lock, and
non-Rust assets stay in the consuming workspace. A syntax bundle preserves
native Rust structure. Portable contracts and compiler-resolved identities use
their own representations.

```sh
oddlang import rust-tree path/to/Cargo.toml package-name bundle
oddlang build rust-tree bundle generated --bundle-sha256 "$reviewed_lock_digest"
```

Both output directories must be absent. The importer uses offline Cargo metadata with `--no-deps` to select one
package and its edition. Locked Cargo checks establish dependency closure
when building the regenerated package. Its input scope is Rust files
under the package root, including tests, build scripts and inactive branches.
Nested packages have their own capture. Compiler inputs outside the capture
scope need separately verified producers.

## Version 1

- `syntax.ttl` holds canonical N-Triples, a Turtle subset. The namespace is
  `urn:oddlang:rust-tree:`. The `bundle` subject records the integer
  `version`, the string `package` and `edition`, and a `file` edge for each
  file. Each file IRI starts with `file:` and ends with its package-relative
  path, with non-unreserved UTF-8 bytes percent-encoded and slashes preserved.
- The file records its `path`, boolean `bom`, and one integer `crlf` per
  original CRLF, at the byte offset of the normalized LF. These fields restore
  the original bytes. Lone CR bytes are passed to the parser.
- Nodes use the file IRI with a fragment numbering the preorder traversal
  from zero. A node has a Rust Analyzer syntax `kind` and outgoing
  `child:0`, `child:1` and subsequent contiguous edges. Leaves also have
  `lexicalKind` and `spelling`. Compound parser tokens use a `token:` branch
  whose children are individually lexable. Trivia and macro token trees
  are part of the traversal. Rust Analyzer 0.0.345 fixes the syntax vocabulary.
- The serializer formats named triples as N-Triples, sorts the UTF-8 lines
  bytewise and writes one LF per statement. The emitter accepts equivalent
  triple order. Duplicate triples, blank nodes, unreferenced syntax,
  noncanonical identities, and unsupported structure are invalid.
- `sources.json` records the version, package, edition and SHA-256 indexed by
  package-relative Rust path. `bundle.lock` pins the inventory bytes and
  canonical triples. The independent `--bundle-sha256` argument pins the lock
  bytes. JSON is serialized with two-space indentation and a final LF.

The emitter independently lexes each leaf in the recorded edition, validates
preorder indices and child membership, and restores file trivia before
checking output hashes. It parses the emitted files again and compares
their canonical graph with the authorized graph. This checks the spelling
and structure together and prevents a series of tokens from being supplied
as one opaque source leaf. It publishes the staged directory only after
every file and the complete graph pass verification.

To authorize a graph edit, review the changed nodes, update the affected
source digests and graph digest, and publish a new independently reviewed
lock pin. Rerun the relevant build-input and behavior checks for the release.
