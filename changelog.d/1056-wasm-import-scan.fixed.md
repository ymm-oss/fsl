Fixed (#1056): the WASM browser-parity harness no longer reads `from "…"`
inside a `//` comment as an import. It now collects a spec's imported files
with a scanner that follows the FSL lexer's comment and string rules, and an
import string that does not close on its own line fails with the file and
line instead of producing a multi-line path.
