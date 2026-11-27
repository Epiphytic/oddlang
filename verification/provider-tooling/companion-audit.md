# Companion script and tool inventory

The latest scan at `df964346e51f69af65e6d8cb7f55ff278ed379e4` found **158 remaining project-authored tools**, comprising 149 Python and nine shell files. The [progress receipt](companion-progress.json) lists the paths added and removed since the detailed baseline below. To reproduce this snapshot, run the same scanner command with the latest scan revision and compare the resulting TSV digest with the progress receipt.

At companion revision `b95fd5161e75c47913a17a2ee3e68e13da5e8748`, **182 project-authored Python or shell files remain** in the tooling migration inventory. They comprise 170 Python verifiers, producers, helpers and tests, nine shell entry points, and three Python adapters for callers of Rust gates. This counts files. Several work together to qualify one contract. It includes separately owned lanes and does not certify their behavior.

The [path table](companion-audit.tsv) gives the original, baseline and current blob identities, role, removal commit and reference locations. The [count summary](companion-audit.json) pins the table digest and repository revisions. Compared with the 382 original companion rows, 329 paths retain the baseline bytes, 12 have changed and 41 have been removed. Fourteen further paths enter through new files or broader suffix coverage. The union contains 396 paths, of which 355 are present.

| Current role | Files | Evidence and scope |
| --- | ---: | --- |
| Original Marmish at the same path and blob | 166 | Includes 159 baseline scripts, four C programs, two headers and one jq program. The 41 inherited ES modules include release and Actions tooling. |
| Modified original tools | 2 | `scripts/install.sh` selects v0.3.0 and its three release digests. `tools/code-ontology/index.py` extends the original classifier to include companion inventory inputs. Its inherited parser/indexer remains external tooling. |
| Target-language consumers | 3 | `core-node/check.mjs` exercises the generated TypeScript checked value including BigInt. The gap-range TypeScript endpoint and its CJS caller exercise the emitted native TS surface. |
| Native observation fixtures | 2 | The journal C fixture and Darwin process observation fixture are linked to local callers in the table. |
| Remaining Python and shell tooling | 182 | Includes tests and the three `workspace_compat.py` files. These adapters still execute in current Python qualification paths. |

At this revision the Git-tracked license is identical to the pinned original. No additional vendored third-party script tree or relocated original script blob was identified in the audited paths. The original Actions package declares external npm dependencies; its checked-in tests and scripts match original Marmish. External Cargo/npm dependencies and original software sources fetched at replay time have separate provenance. The Rust sources for the compiler, consumers and migrated runners and the YAML CI configuration are outside the script file count. Of 29 tracked files under `.github/`, 28 are byte-identical to the original. The one project workflow invokes a remaining Python producer.

## Callers checked at the audited revision

- The manual AMD64 job still invokes `verification/whole/suite/list_cargo.py`. The Rust archived receipt inspector and target comparison can consume recorded results independently of this producer. [Workflow](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/.github/workflows/original-linux-amd64-inventory.yml#L39)
- The canonical engine, engine public-error, prototype constant and canonical-range gates still load the six-member, buffered or public-data Python adapters. The public-data adapter itself loads the engine fieldless choice verifier through a helper whose default script is `verify.py`. [Canonical engine](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/canonical-engine-choices/verify.py#L21) · [Buffered callback](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/prototype-constant/verify.py#L25) · [Range callback](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/canonical-range-data/verify.py#L23) · [Dynamic engine caller](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/canonical-public-data/workspace_compat.py#L35)
- The core native qualifier invokes Node to check the actual generated value module. The gap-range runner compiles `endpoint.ts` and invokes the CJS transport against the generated module. The parent runners and the target-language consumers have distinct entries. [Core caller](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/core-value/verify.rs#L228) · [Gap-range caller](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/gap-range-endpoint/verify.py#L172)
- The stream regeneration shell command remains a documented direct entry. The converged release verifier at this revision loads the public API Python verifier by directory, and invokes the Rust record checker for its field/trait observations. [Stream command](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/oddlang/README.md#L88) · [Directory loader](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/converged-release-manifest/verify.py#L37) · [Rust record checker](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/converged-release-manifest/verify.py#L166)

The table records the first textual anchor per referring file. It covers tracked code, documentation, workflows and configuration, including Python sibling imports, relative paths, unambiguous path suffixes and explicit unittest discovery. Generated ontology indexes are excluded from reference detection. Of the 182 remaining project files, 89 have a code/configuration anchor, 129 have a documentation anchor and 166 have at least one. Variable-based launch commands, directory loaders and test discovery can also reach files with no matching literal. For example, the offline CLI documents its build and observation scripts using a shell directory variable. [Offline CLI commands](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/offline-cli/README.md#L46)

## Stream wire qualification

Two related Rust runners replaced the structural wire recovery and native operation replay scripts. The structural check re-ran source recovery and pinned projection with the historical compiler and compiled the independent Rust 2021 caller. It matched 17 shape observations, 12 canonical envelope pairs and 15 malformed controls. The Rust 2024 operations caller matched all 81 records across independently built upstream and generated libraries. Both report files matched their frozen macOS ARM64 counterparts byte for byte. The operations gate compared the complete caller streams before summarizing long encodings. [Replay evidence](https://github.com/Epiphytic/marmish-oddlang/blob/b95fd5161e75c47913a17a2ee3e68e13da5e8748/verification/whole/stream-wire/native-replay-macos-arm64.json)

The 41 baseline removals span 16 commits. Each removal row identifies its commit so reviewers can inspect the replacement implementation and its evidence or see where producer-only tests were retained. Remaining Python adapters and producers continue to count even when they invoke a completed Rust gate.

## Reproduce the inventory

The Rust scanner reads Git tree entries and blobs at the selected revisions. It includes executable files and interpreter shebangs as well as conventional script extensions, C and C headers, Go and jq. Use an original checkout containing the historical Marmish commit and a companion checkout with the baseline and audited revision available.

```sh
rustc +1.97.1 --edition 2024 -O verification/provider-tooling/companion-audit.rs -o /tmp/companion-audit
/tmp/companion-audit verification/provider-tooling/baseline.tsv "$COMPANION" \
  b95fd5161e75c47913a17a2ee3e68e13da5e8748 "$ORIGINAL" > /tmp/companion-audit.tsv
cmp verification/provider-tooling/companion-audit.tsv /tmp/companion-audit.tsv
```
