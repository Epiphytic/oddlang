# Read-only file integrity implementation plan

**Goal**  Qualify bounded reads and defensive verification of inert local data.

**Architecture**  Project a descriptor policy that checks the opened file, preserving each
observable error role. A subsequent directory contract will link this component to a closed
line grammar and compare the caller's expected digest before parsing.

- [x] Test an unrelated descriptor fixture with runtime, error binding and recovery controls.
- [ ] Recover the baseline maximum link count and a changed count from independent native
  observations, and check the projected result.
- [ ] Replay file admission, byte bounds, optional ownership and permission checks, digest
  verification and I/O error mapping using inert files on both native ARM64 hosts.
- [ ] Qualify a unique-candidate directory selector and read-only grammar composition.
  Credit the original named case only after it runs unchanged against the generated seam.

Use pinned SHA-256 and Unix descriptor providers. Open files read-only and perform no
writes, launches, installation, signing or network effects. Scope observations to inert
files in trusted temporary directories. The profile checks the final path component and
descriptor metadata. Ancestor traversal and concurrent content changes require separate evidence.

Descriptor limits use a checked one-byte lookahead. The original comparison
covers its documented finite parser and file-size range. The Rust projection
rejects a caller limit of `u64::MAX` for bounded reads because it cannot add the
lookahead byte. The projection preflights path types and opens nonblocking, with
a second type check on the descriptor. Hashing stops after the expected length
plus one byte and rejects a length change. These are explicit safeguards beyond
the stable-file comparison, which uses Unix metadata on macOS and Linux ARM64.
The observations do not qualify device access, ancestor traversal or mutation by
a concurrent writer.
