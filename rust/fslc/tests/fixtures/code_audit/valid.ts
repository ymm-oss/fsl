// SPDX-License-Identifier: Apache-2.0

// @fsl.trace {"schema":"fsl-code-trace.v0","requirement_id":"REQ-ACTION","kernel_target":"action:publish","origin_assurance":"source_backed"}
export function publish(): boolean {
  return true;
}
// @fsl.trace {"schema":"fsl-code-trace.v0","requirement_id":"REQ-OUTER","kernel_target":"action:publish","origin_assurance":"generated_from_source"}
// @fsl.trace {"schema":"fsl-code-trace.v0","requirement_id":"REQ-OUTER","kernel_target":"property:invariant:BooleanReady","origin_assurance":"generated_from_source"}
// @fsl.trace {"schema":"fsl-code-trace.v0","requirement_id":"REQ-OUTER","kernel_target":"property:reachable:Published","origin_assurance":"generated_from_source"}
// @fsl.trace {"schema":"fsl-code-trace.v0","requirement_id":"REQ-REACH","kernel_target":"property:reachable:Published","origin_assurance":"generated_only"}
// @fsl.trace {"schema":"fsl-code-trace.v0","requirement_id":"REQ-SAFETY","kernel_target":"property:invariant:BooleanReady","origin_assurance":"unknown"}
