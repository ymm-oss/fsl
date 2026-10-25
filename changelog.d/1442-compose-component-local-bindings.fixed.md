Fixed (#1442): component rewriting now preserves sequential `let` bindings and
pattern scope in guards, branches, and expressions instead of replacing local
reads with prefixed component-state reads. Composed BMC, induction, explicit
exploration, and Monitor replay agree with the standalone component. A pattern
binding that collides with an original component declaration is now rejected
in compose with the same message, kind, and location as the standalone spec;
these invalid compositions previously passed `check` after prefixing hid the
collision. The pattern-collision rule is shared with the ordinary type checker.
