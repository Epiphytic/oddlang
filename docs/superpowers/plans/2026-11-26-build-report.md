# Build metadata as a native command

Recover a portable ordered report of build facts. A field is a stable identity with a wire name and typed value source. The first profile supports unsigned integer and integer-list literals, the built package version, target identifier, and the input revision represented by the release. The release supplies its revision pin. This identifies the represented source contract. The actual Oddlang compiler, companion and executable digests belong in the external build receipt. Output is compact JSON followed by a newline on stdout with a successful exit. Ignore runtime environment overrides. Keep output keys and field order contractual.

- [x] Generate a reusable Rust package component and a checked CLI callback. Read package version from Cargo and capture the actual Cargo target at compilation, including cross-target builds. Lock any generated build script. Reject duplicate field identities and wire names, malformed revisions and unsupported value sources before publication.
- [x] Run an independent unrelated-package fixture and verify byte-for-byte output through the native executable, including a wire-name mutation, changed package version, alternate source revision, target identity and negative controls. Check that old packages retain their lock hashes.
- [x] Bind the two native release-report commands in the companion. Compare stdout, stderr and exit code against the original on both ARM64 hosts, check alternate product metadata without reading application source, preserve prior product locks, and publish exact framework and companion pins. Record any unsupported invocation combinations as gaps.
- [ ] Recover source references separately. The first output contract and static format list are authored. The planned recovery must link the five original format declarations in compiler-checked order.

## Native reference and scope from plan review

Qualify client and daemon `release-info` in their ordinary development layout, for each supported native ARM64 host. Compare both original and generated exit status, stderr bytes and stdout bytes. At pinned original af54339d250290490d62d4475a77539b7e739071 the ordered JSON members are schema (u8), version (the callback's package version), target (Cargo's compilation target), commit (a 40-character lowercase hexadecimal input revision) and formats (exactly five u32 values, profile/journal/engine/config/command). Include escaped JSON keys in the independent generic fixture. The companion selects the native ASCII wire names. Match serde_json compact encoding followed by LF. The report accepts the all-zero revision as upstream does; publication policy is a separate operation.

The original build accepts its compile-time revision override or captures Git HEAD. Qualify its output against the pinned source commit and reject an original that reports another revision. The generated release pins the input revision and seals the graph that contains it. Rebuilding a changed graph changes the generated source. Record the companion source revision, actual compiler revision and executable digests separately. The generated process does not consult Git or runtime overrides for those fields. Check format facts against the pinned source declarations and the observed original executable. Record which declarations have a generated implementation, leaving missing providers explicit.

Compare global flags, including rejected foreground, session and server limits and joint failures; compare usage errors. Qualify the no-startup-reporter environment. Installed-layout checks, descriptor-based startup reporting, broken output pipes, worker exact-argv, and installed-check and installed-fixture metadata entry points each retain separate conformance obligations. Cross-compilation uses Cargo's TARGET; execution on Linux amd64 and custom targets requires separate evidence.

[Native ARM64 qualification](https://github.com/Epiphytic/marmish-oddlang/pull/60)

## Structural recovery

`oddlang recover build-report ROOT SELECTION OUTPUT` reads SHA-256-pinned Rust
modules. The selection identifies ordinary crate roots, the loaded module path,
a named record and its local constructor. Build-role bindings identify which
compile-time strings represent Cargo's target and the represented revision.
A pinned revision remains an input release fact.

The bounded subset recovers derived Serde record fields in declaration order,
with optional literal wire names. It accepts unsigned scalar literals,
compile-time string inputs, and fixed arrays of named integer constants. The
resolver follows plain external module declarations and direct constant
aliases to typed literals across selected crates. Unsupported conditional or
ambiguous definitions fail recovery. A separate ordered reference list keeps
stable constant identities, signedness and width, alias links, and identity,
unsigned widening and modular cast rules. The renderer calculates the list
from these facts. Legacy literal arrays continue to encode as before.

Recovery records candidates and file hashes. Native compiler and runtime
checks establish the external crate name and role binding for each selected
release. They do not qualify arbitrary conditional Rust module graphs, build
scripts, serde customizations or other command behavior.

- [x] Recover and render ordered declarations, and exercise a renamed fixture
  with signed wrapping, unsigned widening, conditional definitions and
  visibility and namespace controls.
- [ ] Compare native original and generated client and daemon outputs, and
  mutate the original engine declaration behind its public alias.
