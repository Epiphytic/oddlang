# Unified recovered vault and journal implementation plan

**Goal:** Qualify the recovered release report, fixed-width vault values and
public journal records together in one source-free native workspace.

**Architecture:** Merge compiler lineages for the existing recovered report,
fixed-value and public record projections. Append the same pinned journal
component to the native member of the #70 companion workspace. Build that
workspace once within the existing source and network isolation and use its
compiled libraries and executables for all three independent oracles.

**Constraints:** Keep the #70 Cargo lock, native dependency edges and
core, stream and engine member pins. Keep the fixed vault member and
source-derived report inputs. Scope remains the selected public data
contracts and release reporting.

- [x] Join the compiler branches and rerun the unrelated public record and fixed-value fixtures.
- [x] Assemble the combined locked workspace with the journal component and check source, Cargo and sibling-member hashes.
- [x] Run the fixed-width vault test and direct ownership probe, all selected native release-info invocations, and the complete named journal caller against the one sealed product.
- [x] Review and publish the compiler branch and companion evidence. Retain all preceding release locks.

The [companion qualification](https://github.com/Epiphytic/marmish-oddlang/pull/76) records all three oracles against the same sealed macOS ARM64 release build from compiler `84b173b288ef41b60c87aa45291f3f20ccb44bed`.
