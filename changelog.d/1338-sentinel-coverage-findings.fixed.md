Fixed (#1338): generated-only sentinel actions no longer appear as dead-action
coverage findings in `fslc ledger`, or as uncovered badges and coverage
denominator entries in `fslc html`. Runtime warnings, ledger, and HTML share the
existing source-location rule through `KernelModel::action_source_span`:
positive authored primary-origin position, then positive kernel declaration
position, never a name/prefix test. Authored blocked actions retain their findings.
Raw coverage, verify/induction/Worker JSON, and HTML Raw Data remain unchanged;
HTML with no eligible coverage entries shows `0/0`. Migration: corrected ledger
and HTML artifacts have changed rendering digests, so existing approvals report
`drifted` (exit 0); review and re-approve the artifacts.
