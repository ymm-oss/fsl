Fixed (#1475): Reject overflowing causal analysis minimum-time sums with the
contributing claim's location, and evaluate causal time thresholds exactly.
Malformed generated document frontmatter/markers, missing chain step files,
and malformed approval timestamps now use their existing error envelopes instead
of panicking. Normal document and approval bytes/digests and source-check
acceptance are unchanged.
