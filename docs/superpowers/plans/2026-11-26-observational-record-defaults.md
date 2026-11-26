# Public data records with fieldwise defaults

Add an unsigned 32-bit field shape and fieldwise default capability to
public records. Add the owned native path type for records that expose
paths as data. A pinned source selection can observe explicit declarations
in a mixed module. Native compilation confirms the selected type binding
and exported path, including when sibling item macros are present.

- [x] Qualify the record capability and unsigned width in an independent
  Rust fixture. Exercise shadowed names, missing path imports, default
  values, and a source field-order mutation.
- [x] Recover `InputReceipt`, `MaintenanceReport`, and `ArtiDirs` from their
  actual pinned files. Compare downstream original and generated callers
  for field types, values, visibility, Debug, Default and Clone.
- [x] Compose the record batch in a sealed workspace and replay both ARM64
  targets with the preceding witnesses and package pins intact.
- [x] Classify the remaining pure data declarations by the reusable
  optional values, collections, record references and choice capabilities
  they need.

On native macOS and Linux ARM64, runtime
`6457c87f65f542d47045d0338a64e99346e1ca39` generated the same sealed
workspace locks and all three records matched their original callers.
The 32 negative checks preserve Rust field widths, export paths and
declared capability boundaries. The original MaintenanceReport
field-order mutation changed Debug output on both sides. The same
workspace replayed MAX_LINE, private input, the engine errors, native
identity and the earlier data witnesses.
