# Ordered field writes

`oddlang recover record-writer SOURCE SELECTION OUT` observes an ordered sequence of byte writes in a pinned Rust module. Select a module-level writer by name, its record type, a stable owner IRI, source SHA-256 and any external tag type IRIs. The writer takes a mutable byte vector and shared record reference. The output directory must be new.

`graph.json` carries field identities, sequence, encoding and type evidence. Field identity follows the wire-schema importer’s owner-and-field convention. Repeated writes stay in order. The supported encodings are fixed bytes, big-endian unsigned integers, boolean bytes, external byte tags and length-prefixed bytes. The Rust length cast records the low 32 bits. A consumer using checked lengths must establish the length bound separately. The graph carries no function body or native library selection.

Recovery checks local helper bodies for integer and length-prefix writes. Unknown control flow, calls, conditional writers, ambiguous declarations and shadowed standard type names fail recovery. The observation is lexical. Its separate witness records the source digest, native binding and unexpanded item macros. External tag assignments and macros require independent evidence. A surrounding hash operation also needs a domain, parameter order, validation and output format. These facts are outside this writer’s body.

The unrelated radio fixture compiles the native writer and compares its output with an independent graph consumer. It changes statement order, renames helpers and supplies unsupported helper bodies. Run it with `python3 -B verification/record-writer/verify.py /path/to/oddlang`.
