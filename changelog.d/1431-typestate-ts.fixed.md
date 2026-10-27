Fixed (#1431): TypeScript typestate skeletons now escape ECMAScript and strict-mode
reserved action and parameter names (including `eval` and `arguments`), keep
function/signature identifiers distinct, allocate action names once per spec in
declaration order for reuse across entities, and keep the phantom generic clear
of every model and generated type name. Parameter types now come from the model
instead of blanket `number`. Enum types whose public names collide with emitted
entity interfaces or state aliases use inline member string-literal unions,
including inside options and sets in parameters and data fields.
These are native/frozen-reference differences in
addition to the earlier #680 guarded state-preserving self-loop report entries
(`state_preserving: true`) and generic signatures preserving the exact input
state. Native judgment also already scopes enum fields by owning struct and
drops disjunctive guards when any disjunct does not constrain the entity. JSON
`typescript` fields carry the TS differences; the command envelope and existing
report field shapes (including source-name `params`) stay compatible, but report
values, entity inclusion, applicability and summary counts can differ under the
earlier judgment rules. Native entity order is alphabetical (the frozen reference
uses declaration order). Neither JSON nor TS is claimed byte-identical to the
frozen reference.
