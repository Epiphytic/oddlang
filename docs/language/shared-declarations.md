# Shared declarations

`oddlang build shared-declarations rust GRAPH.json OVERLAY.json OUTPUT` produces `shared.rs`, a projection lock and portable `obligations.json`. Use the same graph and overlay with the `shared-declarations` native package component to combine independently generated modules. The generator accepts structured declarations with distinct absolute IRIs.

The graph uses `format: "oddlang-shared-declarations-v1"` and an `id`. At least one of the following declaration lists must be nonempty.

| List | Entry fields | Observable meaning |
| --- | --- | --- |
| `constants` | `id`, `value` | Unsigned 64-bit literal, `{"ref":"IRI"}` or `{"multiply":[left,right]}`. References resolve within the graph, multiplication is checked, and cycles fail. |
| `errors` | `id`, `variants` | Each variant has an `id`, a `payload` of `none`, `text`, `u64` or `bool`, and an ordered `message`. A segment is `{"text":"literal"}` or `{"payload":true}`. Payload segments require a payload and render with its default display semantics, including when a Rust caller formats the whole error with width or precision. |
| `results` | `id`, `error` | A result with generic success and a named error type in this graph. |
| `stringEnums` | `id`, `variants` | Each variant has an `id` and a distinct exact `wire` string. The Rust target derives Serde for the native unit enum. |

Duplicate mapping keys are rejected. The Rust overlay uses `format: "oddlang-shared-rust-v1"`. The `names` object maps every declaration and variant IRI to an identifier. The `constantTypes` object binds each constant to `u8`, `u16`, `u32`, `u64` or `usize`. Values must fit fixed-width bindings. A `usize` value is emitted as a typed unsigned literal and the target compiler checks its pointer width. The `variantOrder` object maps each error and enum IRI to an array enumerating every variant exactly once in native declaration order. This profile projects text payloads as borrowed static strings and emits native Display and Error implementations. Source and projection locks are stable under reordering of declaration lists.

Each portable obligation is keyed by declaration IRI. Constant values and unsigned payload samples use decimal strings to retain the full unsigned range in all consumer languages. Error examples include the rendered payload, enums include a rejected wire spelling, variants include their accepted wire spelling, result declarations identify the error type, and the typed overlay supplies the native variant order. The pinned Rust consumer tests independently exercise the generated API.

Run the unrelated meter example with `python3 -B verification/shared-declarations/verify.py /path/to/oddlang`. It checks typed message interpolation, exact native order, Serde wire spelling and rejection, constant reference/product evaluation, deterministic regeneration, the Cargo package's public/crate-private paths and malformed graphs.

A string enum optionally declares a byte representation as `numbers: {"tags": {"variant IRI": 3}, "unknown": {"id":"error IRI","argument":"optional static text"}}`. Every variant requires a different byte. The Rust binding maps each unknown-error IRI under `numberErrors` to a `typePath` and `variant`. The generator emits explicit discriminants, a u8 representation and a checked TryFrom implementation. Its obligation table includes all 256 accepted or rejected inputs independently of the Serde string spellings.
