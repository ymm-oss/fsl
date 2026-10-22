Changed (#9987): the requirements groundwork's process-block rewrite reports,
as an `info` finding `derived_terminal_added`, when the rewritten document
gets terminal states derived from its lifecycles' final stages that the
process block did not declare. The finding names each entity and its final
stages, says that the deadlock verdict may change, and gives the equivalent
`terminal { … }`; the exit status is unchanged. Parsing, diagnostics, and
command output of published documents are unchanged.
