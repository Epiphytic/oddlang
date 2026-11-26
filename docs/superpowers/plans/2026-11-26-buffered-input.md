# Bounded private buffered input plan

**Goal** Recover a selected native buffer's shape and project its reviewed
private read and consume policies.

**Architecture** The compiler adapter provides the observed roles' types
and extents. Seeded semantic transitions determine the native projection.
The package gate verifies the erasure provider and keeps the leaf private.

**Spec** [Bounded private buffered input](../specs/2026-11-26-buffered-input.md)

- [x] Write an unrelated native fixture and reproduce the permissive public-module failure.
- [x] Validate the selected compiler shape, layout, Rust binding and private provider contract.
- [x] Compare independent native outputs for all eight transition combinations and verify a real source mutation.
- [ ] Review and publish the framework branch with the exact evidence.

## Predecessor named-suite evidence

- [x] [Companion #75](https://github.com/Epiphytic/marmish-oddlang/pull/75)
  qualified the registered named cases of the separately built recovered
  five-member workspace using compiler `6e2d1ef`. macOS ARM64 matched 39 of
  909 registered cases and Linux ARM64 matched 40 of 887. Each host recorded
  five ignored cases. This evidence belongs to that workspace. The journal
  successor has its own qualification.
