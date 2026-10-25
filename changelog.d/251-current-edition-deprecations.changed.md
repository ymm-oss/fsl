Changed (#251): current-edition check and verify JSON now warn on legacy declaration string
metadata, colon quantifiers, and inline action mappings, using the existing lint
codes and migration-safety data. The Worker shares these warnings for an explicit
current-edition header. Verdicts and exit codes are unchanged.
