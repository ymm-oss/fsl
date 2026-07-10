# Bounded underspecification findings

Issue: #179

## Goal and epistemic status

`fslc analyze SPEC --profile ai-review` reports places where the specification
still permits materially different answers to the same reachable situation.
These are review questions, not verifier failures. Every finding retains
`formal_status:"not_a_violation"` and names its evidence basis.

The semantic probe is bounded to four transitions. A witness proves the shown
branch is reachable within that bound; absence of a finding does not prove the
specification is fully determined at greater depth.

## `divergent_choice`

A finding requires all of the following:

1. one state is reachable from init while all declared invariants hold along
   the prefix, within depth 4;
2. two distinct non-generated action names have enabled instances in that same
   state;
3. executing each action from that state produces successors where at least
   one user invariant or acceptance `expect` predicate has a different truth
   value.

The solver constructs two successor states from the same symbolic current
state, pins a different action instance into each transition, and asks for an
XOR over contract predicates. The witness includes the reachable prefix, both
actions/parameters, both logical successors, divergent state names, and the
predicates whose outcomes differ.

This is deliberately narrower than “two actions are enabled.” Independent
operations over different entities, or alternatives that all preserve the same
declared contract outcome, are not automatically called divergent choices.

## `unconstrained_effect`

The structural seed is an `unread_state` candidate: a written state variable
whose transitive effect-read chain reaches no guard, property, ensures clause,
or acceptance/forbidden scenario. The bounded probe upgrades that candidate
only when two distinct actions are enabled in the same reachable state and can
produce different next values for that exact logical state variable.

This distinguishes actual next-state freedom from a deterministic audit/history
counter written by only one action. A single deterministic writer keeps the
existing structural `unread_state` review signal because no same-state choice
witness was found.

## Bounded exploration and cost controls

The probe reuses the verifier's `init_constraints`, transition relation,
logical equality, action-instance enumeration, and trace rendering. Path states
are constrained by all invariants. Branch successors are not constrained by
the invariants so `divergent_choice` can expose a choice whose property truth
values split.

Exploration is deterministic:

- depth: 4;
- distinct action-name pairs only (different parameter instances of the same
  action are not treated as separate policy choices);
- generated lowering actions are excluded;
- action names and unconstrained state names are sorted;
- at most 256 action-instance pair queries per spec.

If expression evaluation or the semantic solver cannot build a probe, semantic
findings degrade to an empty set and ordinary structural analysis continues.
The AI-review command must not become an internal error because optional
semantic evidence was unavailable.

## Question output and schema

Both finding types add:

```json
{
  "evidence_basis": "bounded_bmc",
  "spec_question": "Both X and Y are possible ... Which outcome is intended?",
  "witness": {
    "bounded_evidence": {
      "available": true,
      "depth": 4,
      "reachable_at_step": 0
    }
  },
  "candidate_repairs": [
    {"kind": "ask_spec_question", "template": "... ?"}
  ]
}
```

`analysis-findings.v0.schema.json` adds optional `spec_question` (must end in
`?`) and `evidence_basis` (`structural` or `bounded_bmc`). They remain optional
so existing v0 findings are backward compatible.

The question asks the specification owner to decide or explicitly declare that
both outcomes are intended. It never tells an agent to delete an action,
strengthen a guard, or invent a product rule without confirmation.

## Overlap with existing findings

The stronger evidence supersedes its structural approximation:

- A state reported as `unconstrained_effect` is suppressed from
  `unread_state`.
- An unguarded action included in a bounded `divergent_choice` or
  `unconstrained_effect` witness is suppressed from `unguarded_action`.
- If no bounded witness exists, the original structural finding remains; lack
  of a depth-4 witness is not evidence that the structural concern is resolved.

Other findings are not duplicates. For example, `progressless_cycle` asks about
eventual progress, while `divergent_choice` asks which immediate contract
outcome is intended.

## Non-goals

- Proving determinism or completeness at all depths.
- Treating nondeterminism as automatically wrong.
- Comparing parameter instances of the same action.
- Natural-language policy invention or automatic repair.
- Replacing `verify`, acceptance replay, scenarios, or mutation testing.
