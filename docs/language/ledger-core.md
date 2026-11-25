# Ledger core v1

The unit of compilation is the RDF graph formed by the local Turtle import closure of `domain.ttl`. Union removes duplicate triples. Relative import filenames resolve from their importing file inside the project. Prefix names, file order, triple order and blank-node labels have no semantic meaning.

## Vocabulary

In the tables, bare class, predicate, target and type names expand by appending the exact case-sensitive spelling to `urn:oddlang:core:`. Ledger names written as `l:X` expand to `urn:oddlang:ledger:X`. Error names written as `e:X` expand to `urn:oddlang:ledger:error:X`. Class membership uses `http://www.w3.org/1999/02/22-rdf-syntax-ns#type`. String and integer literal datatypes are respectively `http://www.w3.org/2001/XMLSchema#string` and `http://www.w3.org/2001/XMLSchema#integer`.

Each typed node has exactly one class IRI. Each listed operand occurs exactly once unless a cardinality says otherwise. References resolve within the union. A structural node may be named or anonymous. An expression operand denotes a node, never an inline RDF literal. Unlisted properties, unsupported classes and detached declarations are errors. A malformed anonymous operand is identified by its nearest named owner and the path of predicates leading to the error.

| Declaration | Operands and constraints |
| --- | --- |
| Import document | An untyped subject with only `imports` (one or more relative string filenames). Operations and projections may also carry zero or more imports. |
| Entity | Named `l:Account`. `field` points to each named Field (at least the three below). `key` points to `l:id`. |
| Field | Named. `type` points to Text or Integer. All fields are required. The integer field `l:balance` has one untyped `range` node with integer `min` and `max` in signed i64 and min ≤ max. Text fields, including `l:id` and `l:owner`, have no range. The release balance bounds are 0 and 9223372036854775807. |
| Projection | Named. A target projection has `target` (Rust, Sql or Avro), `projects` (l:Account), `name` (string), and one `mapping` node per field. Exactly one target projection exists for each of the three targets. Each untyped mapping has `field` (Field IRI) and `name` (string), with zero or more string `alias` values for Avro. Physical names are unique per projection. A dependency-only projection has no target, name, entity or mappings. |
| Dependency | Each Operation or Projection may have zero or more `dependsOn` references to named Entity, Field, Projection or Operation declarations. Impact follows these references, structural projection relationships and the fields reached by executable reads and assignment targets. |
| Operation | Named. `argument` identifies all three named bindings below. `guard` identifies one or more named Guards. `assign` identifies one or more Assign nodes. |
| Argument | Named. `type` is Text for `l:source` and `l:destination`, I64 for `l:amount`. Each is also a nullary expression returning its call argument. Text IDs are case-sensitive and amount covers every signed i64. |
| Guard | Named. `priority` is a positive integer representable in u64, unique within the operation. `condition` is a Boolean expression. `error` points to a named Error declaration. |
| Error | Named, with a nonempty local name under `urn:oddlang:ledger:error:`. The two commit-time errors AssignmentConflict and FieldOutOfRange are reserved. No operands. A failed guard returns the full IRI. |
| Assign | Structural. `account` identifies a bound text ID argument, `field` identifies a declared non-key field, and `value` is an expression of that field's type. An operation may assign each (argument IRI, field IRI) pair at most once. |

Target identifiers start with an ASCII letter, followed by ASCII letters, digits or underscores. SQL identifiers occupy at most 63 bytes. Rust names must permit compilation alongside generated types, constructors and methods.

| Expression class | Operands | Result |
| --- | --- | --- |
| Exists | `account` is a bound text ID argument IRI | Boolean indicating entry-state membership |
| Read | `account` is a bound text ID argument IRI, `field` a declared Field IRI | Entry-state field value, with the declared type and integer bounds |
| Constant | `integer` is an integer literal representable in signed i128 | Integer with singleton interval |
| NotEqual | `left`, `right` are text expressions | Boolean, case-sensitive inequality |
| Greater, GreaterEqual, LessEqual | `left`, `right` are integer expressions | Boolean from comparing left to right using >, ≥ or ≤ respectively |
| Add, Subtract | `left`, `right` are integer expressions | Integer, left plus or minus right |

Expressions may share subtrees. Each guard condition or assignment value has at most 24 expression nodes on a root-to-leaf path, counting both endpoints. Across all guards and assignments an operation may contain at most 256 expression nodes after expanding shared references once per occurrence. Lowering checks guard roots in priority order, then assignment roots in lexical (account IRI, field IRI) order, visiting left before right. Cycles are invalid. A previously successful whole-guard Exists on the identical argument is required for every Read and assignment target. Integer arguments widen from i64 to i128 before arithmetic. For operand intervals [a,b], [c,d], addition yields [a+c,b+d] and subtraction yields [a−d,b−c]. Reject an expression before execution if either endpoint exceeds signed i128. This conservative check uses the declared intervals independent of runtime guards.

## Execution

Bind source ID, destination ID and amount at call entry. The entry snapshot maps unique IDs to checked accounts with matching keys. In increasing priority order, evaluate each guard against the snapshot. Stop at the first false condition, return its error IRI and preserve the entire map. Once all guards succeed, resolve every assignment target and compute every value against that same snapshot. Check distinct resolved (account ID, field IRI) targets and all field constraints, including balance bounds before narrowing. After all guards pass, check target collisions first. Two assignments to the same resolved (case-sensitive account ID, field IRI) return `urn:oddlang:ledger:error:AssignmentConflict`, even if they compute identical values or a candidate also exceeds its bounds. Assignments to different fields of the same record are permitted. Next, a candidate integer outside the target field’s inclusive declared interval returns `urn:oddlang:ledger:error:FieldOutOfRange`. These two built-in errors apply to every operation. Both preserve the entire map. Text reads include the key and owner, and a Text field other than the key accepts any text value, including the empty string. Commit the checked updates simultaneously and preserve every other field and record.

For the released TransferFunds, guards 1–6 require source existence, destination existence, distinct IDs, positive amount, sufficient source balance and destination capacity. Their errors in order are e:SourceMissing, e:DestinationMissing, e:SameAccount, e:NonPositiveAmount, e:InsufficientFunds and e:DestinationOverflow. The hand-authored [expected lowering](../../tests/fixtures/transfer.lowered) gives full IRIs and operand order.

With balances A=10, B=7 and amount 3, both accounts exist, their keys differ, 3 > 0, 10 ≥ 3 and 7 + 3 fits. The two buffered results are A=7 and B=10. With source A, destination A, no account A in the snapshot, and amount −1, source existence already fails at priority 1. Return `urn:oddlang:ledger:error:SourceMissing` without evaluating destination existence, ID equality, amount or balance reads. The map remains identical to the snapshot.
