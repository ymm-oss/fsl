Changed (#9991): the formatter can respell a requirements document through
the parser's record of where each keyword and phrase was written: keywords
through the internal keyword table, names through the glossary, and the word
order of phrases whose role markers it adds or removes; a respelled document
must lower to the same checked kernel. It also indents the continuation lines
of single-line requirement statements and separates a role marker written
onto its word. Formatting, diagnostics, and command output of published
documents are unchanged.
