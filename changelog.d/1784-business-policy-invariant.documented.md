Documented (#1784): `docs/manual/LANGUAGE.md` and `LANGUAGE.ja.md` §13.3
and the fsl-business skill now describe the `business` form
`policy ID "text" [satisfies CTRL, ...] invariant { expr }`. It is one kernel
invariant named `ID`, with the policy as its `requirement`, and nothing else is
synthesized. The documentation lists the names the body can read: `stage(c)`,
the stage map `<entity>_stage`, the enum `<Entity>Stage`, and the history flag
`<entity>_stage_via_<W>[_<W>...]`, which exists only while a no-bypass policy
over those waypoints is declared. The design note's example
(`DESIGN-dialects.md` §3.1, now an `fsl check` fence) replaces a
`… => true` tautology with a policy that reads that flag to say that an
approval is final; the skill example gains the same rule as `POL-RET-003`.
`rust/fslc/tests/issue_1784_business_policy_invariant.rs` verifies and proves
both examples, finds the policy violated when an `Approved -> Rejected`
transition is added, and rejects the flag once the no-bypass policy is removed.
