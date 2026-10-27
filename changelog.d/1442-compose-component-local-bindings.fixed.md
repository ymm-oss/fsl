Fixed (#1442): component rewriting now preserves model-normalized `let` bindings
and pattern scope in logical and guard continuations instead of replacing local
reads with prefixed component-state reads. The tested use-only compositions
preserve standalone local-binding behavior across BMC, induction, explicit
exploration, and Monitor replay. Guards see preceding guard bindings; body
statements and ensures see all guard bindings even when written before a `let`
or `requires`. A pattern
payload is visible on the right of `and` or `=>`, and a `requires` also exposes
it to later guards and the normalized body/ensures; neither statement nor
expression `if` introduces it into a branch. A pattern
binding follows the same phase-aware collision rule as the standalone spec.
Patterns sharing a normalized non-typed `count` or `sum` binder are accepted
in compose just as they are alone; non-expanding `forall` and `unique` forms
retain their collision refusal. Other original-source collisions remain
rejected instead of being hidden by prefixing. The ordinary type checker owns
this decision for both source rewriting and checked expressions. Component-owned
late diagnostics retain the original cause and component position inside the
existing use-site diagnostic wrapper.
Private lets, patterns, quantifiers, aggregates, and statement-forall binders
share one hygienic rewrite across synchronized sources. It inventories both
reads and declarations, including non-exported pattern labels, and preserves
each source's original collision refusals. Header arguments are inserted in
their header-parameter scope, not under another source's guards. Ambient state
or enum names cannot falsely reject private component patterns; an unread
ambient state alone does not force an ordinary let to be renamed.
The same hygiene now covers component property, temporal, initialization, and
inline-state-initializer roots. Bare component enum reads retain their owning
enum when an unrenamed public value would capture them, while uncontested
reads retain their existing representation. Reified bare enum reads print their
source member spelling; conversion, abstraction, and explicitly qualified enum
literals retain owner qualification through node-local spelling provenance.
A single inserted argument pattern without foreign interference
is no longer renamed merely because its name is reserved for freshness.
When sync composition must rename colliding private bindings, synthesized names
such as `__u_substitution_0` may intentionally appear in later error messages,
`explain`, `analyze`, and approval HTML output. Inputs needing no such renaming
keep their existing output spelling.
