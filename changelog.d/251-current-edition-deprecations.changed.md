Changed (#251): current-edition check, verify, and Worker JSON now
warn on legacy declaration string metadata, colon quantifiers, and inline action
mappings, using the existing lint codes and migration-safety data. Verdicts and
exit codes are unchanged; applicable Worker error envelopes also carry the native
implicit-initial-value warnings.
Testgen's verifier-failure envelopes use the same source-edition finalization
as CLI verify, preserving its warnings without changing failure verdicts or exits.
