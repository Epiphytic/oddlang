# Ledger interoperability

A v1 JSON reader accepts a v2 account by ignoring unknown fields. A v2 JSON reader requires currencyCode for current accounts. Historical v1 accounts can be upgraded explicitly with currencyCode set to CAD. Both JSON versions use owner. Account balances and transfer amounts use canonical decimal strings within their declared bounds.

A v2 Avro reader accepts v1 snapshots using the owner alias on ownerName and historical CAD default. In the reverse direction the Avro specification gives aliases to the reader: a v1 reader has no alias or default for its required owner field, so a v2 writer is incompatible. The pinned Arrow Avro reader also accepts writer-side aliases. Its success on this reverse case is an implementation extension and does not establish portable compatibility.

The transfer examples exercise v1 behavior in both bundles. Each request starts with the published initial state. Failed requests preserve that state. The v2 account schema adds currency. Executing transfers between v2 currencies is outside this release.
