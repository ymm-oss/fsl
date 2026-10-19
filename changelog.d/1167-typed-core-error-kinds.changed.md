Changed (#1167): core diagnostics are classified by a typed error kind instead
of their message text, so a new kind cannot reach the CLI, the language server,
or the browser Worker without stating its public `kind` and hint. Diagnostic
output and exit codes are unchanged.
