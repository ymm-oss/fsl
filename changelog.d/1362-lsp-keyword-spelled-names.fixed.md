Fixed (#1362, #552): in the language server, a name spelled like a word the
index used to treat as a keyword (`status`, `count`, `age`, `evidence`,
`owner`, `rejected`, `severity`, `respond`, ...) is now a name like any other
in `spec`, `requirements` and `compose` documents: a state variable, struct
field, enum member, parameter or `let` binding so named is declared, and
go-to-definition, references, rename, hover, document symbols and completion
reach it and its uses (for example `respond(0)` in an acceptance). The
document index now takes those dialects' declarations from the parsed AST.
As a result, the `until` operator inside an `until NAME { P until Q }`
property no longer declares the first name of `Q` as a property, so uses of
that state variable resolve to its declaration again. Other dialects and the
server's other responses are unchanged.
