Fixed (#1416): preserve lossless token/trivia text and Unicode source positions while computing
trivia positions incrementally instead of rescanning source prefixes. Make the
AI project block scanner ignore `//` comment contents during header and brace
matching, preventing ghost declarations, truncated fields, missing siblings and
false unterminated-block diagnostics, without repeated prefix-depth scans.
