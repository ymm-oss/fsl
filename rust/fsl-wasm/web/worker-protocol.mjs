// SPDX-License-Identifier: Apache-2.0

export function workerMessageError(data) {
  if (!data?.transportError) return null;
  const { kind, message } = data.transportError;
  return new Error(`${kind}: ${message}`);
}
