Changed (#9999): requirements lowering runs a name-resolution step first, and
the name owner keeps a per-document table of display spellings beside the
canonical names. Parsing, diagnostics, and command output are unchanged.
