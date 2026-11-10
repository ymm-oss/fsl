// SPDX-License-Identifier: Apache-2.0

export class ProbeTimerExpiredError extends Error {}

export function exitStatusFor(error) {
  return error instanceof ProbeTimerExpiredError ? 124 : 1;
}
