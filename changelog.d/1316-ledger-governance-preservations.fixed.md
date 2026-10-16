Fixed (#1316): `fslc ledger` on a governance document now states, in its
Markdown body, the verdict its exit code reflects. A new `## governance
preservation` section, between the header and the risk list, gives the
top-level verdict (`検証判定`, the `verify` result after the #1303 fold) and
every `preservation` in declaration order with its `before` → `after`, the
controls it preserves, and its own `result`, 🟢 for `refines` and 🔴 for
anything else. Before this, a failing preservation made `fslc ledger` exit 1
while the body showed neither the verdict nor any preservation result, so a
reader of the ledger alone saw no reason for the failure. Migration: only
ledgers of governance documents change; every other ledger is byte-identical.
A governance-document ledger approved with `fslc approval create --kind
ledger` before this release no longer matches its recorded artifact digest,
so `fslc approval check` reports it `drifted` (exit 0); review the new
section and re-approve it.
