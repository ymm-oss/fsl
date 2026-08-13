Required (#781): native CLI error-envelope parity now derives every executed
failure-class cell from the command registry, rejects `SpecPath` entries with
no executable coverage, and requires reasons for non-applicable Markdown
coverage. It includes sidecar-backed AI, causal, database, and approval
frontend commands; validates location shape; and checks that self-retiring
pins exactly match executable registry cells.
