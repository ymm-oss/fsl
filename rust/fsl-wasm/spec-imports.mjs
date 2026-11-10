// SPDX-License-Identifier: Apache-2.0

// Sibling-file paths a spec imports, for the browser parity harness (#1056).
//
// Mirrors the token rules of rust/fsl-syntax/src/lexer.rs rather than
// matching raw text: `//` to end of line is trivia (there is no block-comment
// form), a string literal is `"` ... `"` with no escapes and may not contain a
// newline, and an identifier is `[A-Za-z_][A-Za-z0-9_]*`, or, under the wider
// identifier rule the lexer applies to a next-edition document, `_` or
// XID_Start followed by XID_Continue other than the katakana middle dots
// U+30FB / U+FF65. The wider rule is used for every document: a document the
// ASCII rule lexes has only ASCII identifiers, which both rules read alike, and
// one with any other identifier character is a lexer error under the ASCII
// rule anyway. Characters are read by code point, so a character outside the
// BMP is one character, as in the lexer. Every import form in
// the grammar puts the path string directly after the identifier `from`
// (`use`, `uses`, `implements`, `delegates`, `before`, `after`) or
// `refinement` (`checked_by refinement`), with only trivia in between, so a
// string token whose previous token is one of those identifiers is an import.
// `//` inside a string literal is string content, not a comment.
const STRING = /"([^"\n]*)"/uy;
const IDENT = /(?:(?![\u{30fb}\u{ff65}])\p{XID_Continue})+/uy;
const IDENT_START = /^[_\p{XID_Start}]$/u;

export function specImports(source, fileName) {
  const imports = [];
  let previousIdent = null;
  let line = 1;
  let i = 0;
  while (i < source.length) {
    const ch = String.fromCodePoint(source.codePointAt(i));
    if (ch === "\n") {
      line += 1;
      i += 1;
    } else if (/\s/u.test(ch) || (i === 0 && ch === "\u{feff}")) {
      i += 1;
    } else if (source.startsWith("//", i)) {
      while (i < source.length && source[i] !== "\n") i += 1;
    } else if (ch === '"') {
      STRING.lastIndex = i;
      const match = STRING.exec(source);
      if (match === null) {
        throw new Error(`${fileName}:${line}: unterminated string literal (a string may not span a line)`);
      }
      if (previousIdent === "from" || previousIdent === "refinement") imports.push(match[1]);
      previousIdent = null;
      i = STRING.lastIndex;
    } else if (IDENT_START.test(ch)) {
      IDENT.lastIndex = i;
      [previousIdent] = IDENT.exec(source);
      i = IDENT.lastIndex;
    } else {
      previousIdent = null;
      i += ch.length;
    }
  }
  return imports;
}
