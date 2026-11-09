Fixed (#1755): `fslc migrate --write` keeps a document's line endings and byte
order mark. On a CRLF document, the lines it inserted or rewrote ended in LF
while the other lines kept CRLF, and a document the formatter rewrote (string
metadata, inline maps, domain enums, colon quantifiers) came back LF-only
without its UTF-8 byte order mark. Every line of the written document now ends
as the input's first line does (CRLF or LF), the mark is kept exactly when the
input has one, and the dry run's replacement holds the same bytes. LF inputs
are written as before. `fslc fmt` is unchanged: it still prints LF without the
mark.
