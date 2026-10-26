Changed (#1338, representative decision 2026-10-26): ledger risk rows now include
every non-`refines` governance preservation, attributed using the model's catalog
requirement metadata, and every failed inline `implements` as a spec-level
finding. `unknown_cti` is an unestablished induction finding, with its full
CTI/hint/rank-failure evidence retained, not a reachable counterexample.
Direct `ledger` and `html` now return verification `error` as JSON and exit 2/3
instead of Markdown/HTML or a generated-artifact envelope, and create no `-o`
artifact; HTML stops before explain. Non-error failure reports still render.
The approval-only ledger digest-rerendering path does not use the new gate.
Added findings and corrected coverage rendering change affected approved ledger
and HTML digests (`drifted`, exit 0); corrected declaration attribution also changes
affected ledger digests. Review and re-approve those artifacts.
Documents whose verification errors now prevent report generation must be fixed
before a new artifact can be reviewed. The inline section's noninterference
claim applies only to that section, not to these separate rendering changes.
Verdict findings now follow their actual declaration owner, including ranked
and bounded leadsTo, transitions, action obligations, and synthetic property checks.
Public compose names, generated identities behind authored labels, and branch
names resolve through the core name map before metadata lookup; ambiguous names
remain spec-level rather than guessing a requirement.
Direct ledger verification errors also precede ancillary replay/evidence errors.
Each ledger appendix finding omits only raw top-level `cost` and `cache` execution
metadata, preserving proof evidence and same-named domain fields. Failed-ledger
approval was already non-reproducible for `violated` reports on main; CTI findings
exposed the same issue. Their rendered bytes and unchanged approval round trips
are now reproducible, while genuine reviewed-content changes still cause drift.
