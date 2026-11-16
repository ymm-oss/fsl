Changed (#461): Calibrate the liveness symmetry regression test to require
more than 5% fewer propagations using measured bundled and native Z3 results,
and remove temporary calibration output while retaining counts on failure.
Dual-call-site ablation remains a rejecting control on bundled Z3; its native
Z3 ratio is unverified.
