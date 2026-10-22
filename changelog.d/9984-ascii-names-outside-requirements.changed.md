Changed (#9984): under the identifier groundwork, a name that is not ASCII
in a document without a glossary (every dialect but requirements) is
rejected as `canonical_name_not_ascii`, as an unaliased name in a
requirements document already is. Parsing, diagnostics, and command output
of published documents are unchanged.
