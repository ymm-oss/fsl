Changed (#9999): the formatter, migration, lint, and language server share one
edition type, and the core library and the shared command modules take
explicit parse options. Parsing, diagnostics, and command output are
unchanged.
