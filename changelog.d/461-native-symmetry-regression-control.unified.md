Unified (#461): the liveness symmetry performance regression test compares
symmetric and plain twins on the current Z3 backend, requiring more than 5%
fewer propagations, instead of a bundled-Z3-specific absolute threshold; the
bound is calibrated from measured bundled and native Z3 results, and the
counts are printed on failure. Dual-call-site ablation remains a rejecting
control on bundled Z3; its native Z3 ratio is unverified. Test-only; product
behaviour is unchanged.
