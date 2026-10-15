Documented (#1304): `skills/fsl-design/SKILL.md` now says to declare, for each
failure the design must exclude, the property it would break (with the
construct that fits it), and to model transaction boundaries, queues and
outboxes, and retries as state and actions; that a bounded `verified` says
nothing past its depth and domain sizes, with routes to `--engine explicit`
and "Domain sizing"; and that a design change re-runs `verify` and `refine`
plus `fslc diff --forbid behavior_removed` (also added to the Review/change
row of `skills/fsl-delivery/SKILL.md` Stage Gates). `skills/fsl/references/impl.md`
§9 and fsl-design Workflow 7 add that generated tests and conformance vectors
do not enumerate interleaving or crash points inside one action, so a
procedure the design declares atomic needs implementation tests at the
implementation's own transaction boundaries.
