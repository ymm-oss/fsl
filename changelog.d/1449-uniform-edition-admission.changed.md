Changed (#1449): Native checking and verification classify the root source's
edition before consumption. Verify-shaped results share migration-warning
finalization after governance, including nested verification and report baselines,
instead of repeating it in the CLI verify and test-generation failure paths.
Edition-migration ledger rows now say 移行候補 with their code and message instead of vacuity or mutate advice.
Kernel, conformance, testplan and typestate consumers also admit their captured
source before analysis; closed contracts and raw TypeScript deliver migration
warnings on stderr without changing their output schemas.
Worker check and verify reject unsupported source editions before model or solver
consumption, retain verification-option error priority, and deliver admitted
warnings once after governance.
