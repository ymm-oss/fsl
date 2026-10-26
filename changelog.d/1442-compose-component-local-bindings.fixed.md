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
binding that collides with an original component declaration is now rejected
in compose with the same message, kind, and location as the standalone spec;
these invalid compositions previously passed `check` after prefixing hid the
collision. The pattern-collision rule is shared with the ordinary type checker.
Synchronized component lets are renamed when needed to avoid sync additions'
free names or final guard bindings, as well as sync parameters and arguments.
