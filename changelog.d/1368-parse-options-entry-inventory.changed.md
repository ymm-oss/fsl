Changed (#1368): the parser's entry points take explicit parse options, and a
test inventories every parse entry in the workspace so a new one cannot bypass
them. Parsing, diagnostics, and command output are unchanged.
