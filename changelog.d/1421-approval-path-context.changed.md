Changed (#1421, AP1/AP2): New approval records use unsigned v5 or signed v6
with an absolute creation-directory base and the original source label. Checks
resolve stored relative inputs and reviewed document targets from that base
while preserving literal paths, rendering labels and bytes. Legacy v1–v4
records retain their closed formats, signatures and invocation-relative paths;
older binaries reject new records as unsupported schemas. Checkout relocation
and document source-label admission remain unchanged.
