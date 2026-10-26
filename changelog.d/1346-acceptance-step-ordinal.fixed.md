Fixed (#1346): requirements acceptance steps now report `wrong_kind_argument`,
a top-level `message` identifying the argument and expected type and explaining
that no guard was evaluated, a hint, and the step location for arguments of the
wrong parameter kind, matching forbidden's diagnostic style without changing
acceptance's error kind or exit status.
Correct the language and skill references: enum step arguments take member names,
not numeric ordinals (`paint(1)` is not `paint(Blue)`).
