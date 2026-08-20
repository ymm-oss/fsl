// SPDX-License-Identifier: Apache-2.0

// These are actual outputs captured from historical writers, not reconstructed
// test text. Regeneration deliberately requires the source commit named below:
// use the recorded capture command only after making that original object
// available, then update its writer/output digests in the same review.
export const HISTORICAL_SUMMARY_FIXTURES = Object.freeze([
  Object.freeze({
    id: "original-unqualified",
    provenance: Object.freeze({
      writerCommit: "cbb00dca5acf99742743a22dd33affa29378d85e",
      writerSha256: "3dc17ad18cd035bb9ef197742e283d47e4d6d00169941255aef837cb673185e9",
      captureCommand: "node .github/scripts/capture-cache-budget-audit-summary-fixture.mjs cbb00dca5acf99742743a22dd33affa29378d85e",
      outputSha256: "6ea77d6d6b95de9f6acbfec0a00cfd4d13aae0135d436969aa1de34d2937d649",
    }),
    body: `<!-- cache-budget-audit-occurrence-summary -->
<!-- cache-budget-audit-occurrence:42:1 -->
<!-- cache-budget-audit-cursor:13:1 -->
Detailed recurrence comments are capped at 20; this rolling summary records the latest recurrence.
Coalesced failed attempts: 1.
Recent coalesced identities: 41:1.

- Workflow run: [42](https://github.com/ymm-oss/fsl/actions/runs/41)
- Trigger: \`schedule\`
- Conclusion: \`failure\``,
  }),
  Object.freeze({
    id: "interval-qualified",
    provenance: Object.freeze({
      writerCommit: "0237fb1fe2b30911ddd5cdf60de1020810e72164",
      writerSha256: "d8618cd5d2bf8d8c99c8b0093d7e55b34fd37240f71ee30cfed32a85bea90833",
      captureCommand: "node .github/scripts/capture-cache-budget-audit-summary-fixture.mjs 0237fb1fe2b30911ddd5cdf60de1020810e72164",
      outputSha256: "81b01087c076b23e2a804ff0398d62b84017780d73a07b0d72f113fbe12b1ddf",
    }),
    body: `<!-- cache-budget-audit-occurrence-summary -->
<!-- cache-budget-audit-occurrence:42:1 -->
<!-- cache-budget-audit-cursor:13:1 -->
This rolling summary records coalesced failures and the latest recurrence.
Observable coalesced failed attempts in this summary interval: 1.
Recent observable coalesced identities: 41:1.

- Workflow run: [42](https://github.com/ymm-oss/fsl/actions/runs/41)
- Trigger: \`schedule\`
- Conclusion: \`failure\``,
  }),
]);
