# Balance impact experiment

Both encodings induce eight distinct dependency edges among stable IDs. Account has three fields, three target projections reference Account, TransferFunds reads and assigns balance, and an unrelated projection depends on owner. The query reports operations and projections. Both implementations return Avro, Rust, Sql and TransferFunds in that order.

| Measurement | Turtle | Stable-ID TOML |
| --- | --- | --- |
| Authored edges between named concepts | 7 plus the balance references in executable reads and assignments | 8 |
| Duplicate dependency declarations required | 0 | 0 |
| Add the Audit projection downstream of Sql | Declare its type and one edge in a new file, then add one import | Add the ID to the report list and one dependency entry in the flat fixture |
| Application-maintained transitive traversal | One SPARQL property path over structural links, executable operands and explicit dependencies | One fixed-point loop scanning dependency entries until the reached set stops growing |

The Turtle experiment reads seven reachable files, including a diamond through fields.ttl. An unimported Audit file changes nothing. Import it from sql.ttl and Audit joins the result. The Turtle implementation maintains a local import loader, copies its merged graph into an Oxigraph store for the query, and brings in the RDF and SPARQL dependency tree. The TOML comparison uses the equivalent merged edge set. Its test measures traversal after loading that single file. It does not measure a TOML import resolver or claim a speed advantage for either format.

Retain Turtle for the behavior experiment. Turtle derives the transfer’s field dependency directly from the executable operands. SPARQL also supplies transitive traversal. The application only binds the start IRI and reads ordered solutions. This meets the design's traversal criterion. The TOML implementation is short, and this fixture provides no evidence for a general authoring-cost or performance advantage.

Run `cargo test --locked --offline --test impact` to reproduce the comparison.
