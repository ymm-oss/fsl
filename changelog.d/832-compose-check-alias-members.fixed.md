Fixed (#832): native `fslc check` now fails closed for two compose alias
resolution gaps that `verify` already rejected. A declared alias no longer
authorizes a nonexistent qualified type such as `core.NoSuchType`, and an
undeclared alias-shaped expression in an `init if` condition receives the same
type check as an assignment right-hand side. Both errors retain the author's
spelling and source location; valid imported types and state conditions remain
accepted.
