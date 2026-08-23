Fixed (#873): generated cargo-mutants liveness exclusions now derive their
line-scoped expressions from maintained source anchors instead of requiring
hand-copied line numbers, and their freshness check is portable across CRLF
checkouts.
