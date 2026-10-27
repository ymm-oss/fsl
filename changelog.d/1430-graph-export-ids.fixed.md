Fixed (#1430): Graph exports now re-check suffixed Mermaid identifiers to keep
nodes and edges distinct after sanitization. Analysis and causal exporters share
one escaping rule per format, including DOT quotes, backslashes and newlines,
and Mermaid label delimiters.
