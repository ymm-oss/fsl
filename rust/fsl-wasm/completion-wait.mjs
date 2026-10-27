// SPDX-License-Identifier: Apache-2.0

import { ProbeTimeoutError } from "./browser-outcome.mjs";

// Passing runs 40365947643 attempt 2 and 40397010393 attempt 2 took 133s
// and 211s for the whole test-browser.mjs command; failed runs gave up at
// about 171s. The old poll budget sat inside the normal run-time spread.
// 360s keeps margin while staying far under the 30-minute job cap (#1496).
export const completionDeadlineMs = 360_000;

export async function waitForCompletion({
  evaluate,
  deadlineMs = completionDeadlineMs,
  intervalMs = 250,
  now = () => performance.now(),
  sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
}) {
  const started = now();
  const deadline = started + deadlineMs;
  let details;
  // An in-flight evaluation retains its own CDP timeout; its elapsed time
  // consumes this budget rather than granting another observation-count loop.
  while (now() < deadline) {
    try {
      details = await evaluate();
    } catch (error) {
      if (error instanceof ProbeTimeoutError && error.mode === "cdp") {
        error.message += `; completion wait elapsed ${((now() - started) / 1_000).toFixed(3)}s; `
          + `last phase: ${details?.phase ?? "unknown"}`;
      }
      throw error;
    }
    if (details?.done === "true") return details;
    const remaining = deadline - now();
    if (remaining <= 0) break;
    await sleep(Math.min(intervalMs, remaining));
  }
  throw new ProbeTimeoutError("poll",
    `browser probe timed out after ${((now() - started) / 1_000).toFixed(3)}s; `
    + `last phase: ${details?.phase ?? "unknown"}; last observation: ${JSON.stringify(details)}`,
  );
}
