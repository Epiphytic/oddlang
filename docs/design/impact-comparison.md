# Balance impact experiment

Both encodings represent eight directed edges among stable IDs. Account has three fields, three target projections reference Account, TransferFunds reads balance, and an unrelated projection depends on owner. The query reports operations and projections. Both implementations return Avro, Rust, Sql and TransferFunds in that order.

| Measurement | Turtle | Stable-ID TOML |
| --- | --- | --- |
| Authored dependency edges | 8 | 8 |
| Duplicate dependency declarations required | 0 | 0 |
| Add the Audit projection downstream of Sql | Declare its type and one edge in a new file, then add one import | Add the ID to the report list and one dependency entry in the flat fixture |
| Application-maintained transitive traversal | One SPARQL property path over three predicates | One fixed-point loop scanning dependency entries until the reached set stops growing |

The Turtle experiment reads six reachable files, including a diamond through fields.ttl. An unimported Audit file changes nothing. Import it from sql.ttl and Audit joins the result. The TOML comparison uses the equivalent merged edge set. Its test measures traversal after loading that single file. It does not measure a TOML import resolver or claim a speed advantage for either format.

Retain Turtle for the behavior experiment. Both forms avoid repeated edge declarations, but SPARQL supplies transitive traversal. The application only binds the start IRI and reads ordered solutions. This meets the design's traversal criterion. The TOML implementation is short, and this fixture provides no evidence for a general authoring-cost or performance advantage.

Run `cargo test --locked --offline --test impact` to reproduce the comparison.
