Documented (#1300): the distributed skills now say that a rejection rule the
source states is carried by a checked declaration and a `forbidden` trace, not
by prose alone (`skills/fsl-requirements/SKILL.md` Guardrails,
`skills/fsl/references/layers.md`); that implementation conformance checks the
implementation against the spec, not the intent, so an agent loop is handed
over only after the spec is confirmed against intent, classifies each
divergence before editing code, and does not lower the depth or skip vectors
to reach zero (`skills/fsl/references/impl.md` §9, "Using conformance as an
implementation loop for an AI agent"); and that such a divergence against an
unconfirmed spec is a human decision gate (`skills/fsl-delivery/SKILL.md`).
