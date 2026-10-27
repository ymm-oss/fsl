Changed (#1449): Native checking and verification classify the root source's
edition before consumption. Verify-shaped results share migration-warning
finalization after governance, including nested verification and report baselines,
instead of repeating it in the CLI verify and test-generation failure paths.
Edition-migration ledger rows now say 移行候補 with their code and message instead of vacuity or mutate advice.
Migration-only rows use status 🟡 移行候補 and risk —; non-migration or mixed rows retain 🔴 要確認 and risk 要確認.
Kernel, conformance, testplan and typestate consumers also admit their captured
source before analysis; closed contracts and raw TypeScript deliver migration
warnings on stderr without changing their output schemas.
Worker check and verify reject unsupported source editions before model or solver
consumption, retain verification-option error priority, and deliver admitted
warnings once after governance.
Editor diagnostics reject unsupported editions before model or analysis work and
retain migration finding ranges for UTF-16 publication.
Replay, explain and single-source analysis also apply admission before consuming
the captured source, with readable and graph output warnings kept on stderr.
Domain check now finalizes each result once instead of duplicating migration
warnings across its specialized result and nested verification result, including
non-definitive verification errors; requested editions and read-error kinds are retained.
