Changed (#1390): the migrator can rewrite a requirements `process` block into
the declarations the requirements desugaring reads back into the same
process, check that the rewrite keeps the location-free checked kernel, and
refuse a block it cannot rewrite with a stated reason, writing no file. The
migration's kernel comparison moved from the command into the migration
module. Parsing, diagnostics, and command output of published documents are
unchanged.
