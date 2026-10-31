Fixed (#1640): reserved-name diagnostics for legacy requirement, acceptance,
and forbidden IDs point to the first written ID token, including IDs with
spaces around hyphens, rather than matching description text or the declaration
keyword. ID normalization, accepted spellings, zero padding, declaration spans,
and the public Kernel/JSON projection are unchanged. The public Rust
`RequirementsItem::{Requirement, Acceptance, Forbidden}` variants gain an
`id_span: Span` field; downstream constructors and exhaustive field patterns
must account for this source-compatibility change.
