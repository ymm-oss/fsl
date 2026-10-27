Fixed (#1496): WASM CI retries each browser probe once only on timeout status
124, retaining attempt history in the step summary and never retrying parity
violations or other failures. Only the z3js spike's result-timer expiry now exits
124; FSL browser completion uses a 360-second wall-clock deadline with timestamped
phase diagnostics instead of a fixed poll count. Parity comparison and outcome
JSON contracts are unchanged.
