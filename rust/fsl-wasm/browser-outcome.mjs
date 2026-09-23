// SPDX-License-Identifier: Apache-2.0

export const probeTimeoutExitStatus = 124;
export const parityViolationExitStatus = 65;

export class ProbeTimeoutError extends Error {
  constructor(mode, diagnostic) {
    super(diagnostic);
    this.mode = mode;
  }
}

export class ParityViolationError extends Error {
  constructor(report, reportPath) {
    super(reportPath ? `${report}\nfull report: ${reportPath}` : report);
  }
}

export function classifyOutcome(error) {
  if (error instanceof ProbeTimeoutError) {
    return { outcome: "probe_timeout", status: probeTimeoutExitStatus, mode: error.mode };
  }
  if (error instanceof ParityViolationError) {
    return { outcome: "parity_violation", status: parityViolationExitStatus };
  }
  return { outcome: "harness_failure", status: 1 };
}
