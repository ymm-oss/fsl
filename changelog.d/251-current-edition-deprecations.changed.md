Changed (#251): current-edition check, verify, and Worker JSON now
warn on legacy declaration string metadata, colon quantifiers, and inline action
mappings, using the existing lint codes and migration-safety data. Verdicts and
exit codes are unchanged; applicable Worker error envelopes also carry the native
implicit-initial-value warnings.
