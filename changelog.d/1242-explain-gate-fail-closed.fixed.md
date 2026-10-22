Fixed (#1242): `fslc explain` (and `--readable`) no longer reports
`result: "explained"` with exit 0 and no witnesses for a spec whose
acceptance / forbidden traces the requirement-trace gate refuses. It reports
the gate's own error (`kind: "acceptance"`, `"forbidden"`, or
`"forbidden_setup"`, the envelope `check` and `scenarios` report) with exit 2.
`fslc html` stops at the same error: its exit status stays 2, but it now
prints the gate's error and writes no report instead of a report with an
empty witness section. Migration: a pipeline that read `explain`'s witnesses
or `html`'s report for such a spec gets the gate's error instead; fix the
acceptance / forbidden case it names (the same error `fslc check` reports).
