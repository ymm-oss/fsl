# Verified auxiliary lemma candidates for k-induction

Issue: #177

## Goal

`fslc verify FILE --engine induction --lemma "EXPR"` makes the CTI repair loop
machine-checkable. An agent or human may propose an auxiliary invariant, but
the verifier never assumes an unproved expression. Each repeatable candidate is
first judged as its own safety property; only candidates that reach `proved`
may strengthen the original induction premise.

This is deliberately not an `--assume` feature. There is no path that injects
an unverified premise into a proof.

## Candidate parsing and independent proof

`--lemma` accepts one ordinary FSL expression. The standalone expression parser
uses the same grammar and AST transformer as invariant bodies. A candidate is
normalized as a synthetic invariant named `AuxiliaryLemmaN` (with a
collision-free suffix if the source already uses that name).

Each candidate is checked independently in a derived proof view containing:

- the original state, init, actions, types, and implicit type-bound invariants;
- the candidate as the only user invariant;
- no original user invariants, `trans`, `leadsTo`, or `reachable` properties.

The ordinary induction engine performs the base BMC and step case. Therefore:

- `proved` means the candidate holds from init and is k-inductive on its own;
- `violated` rejects it with a reachable counterexample;
- `unknown_cti` rejects a true-but-not-inductive candidate with its CTI;
- parse/type/semantic failures reject only that candidate and do not abort
  later candidates.

Removing the original user invariants from candidate adjudication is stronger
than circularly proving a lemma under the target property. It ensures every
premise later added to the target proof already has an independent proof.

## CTI-guided retry

After adjudication, the original proof runs unchanged except that independently
proved candidates are available as CTI probes. When it returns an invariant or
`trans` CTI, the verifier evaluates each candidate in the same Z3 model at each
CTI state. A candidate excludes that CTI exactly when its expression is false
at one or more states.

The first proved candidate (CLI order) that excludes the current CTI is added
to the induction/base invariant set, and the original property is retried. The
loop repeats, allowing later candidates to eliminate later CTIs. A candidate
that was proved but never needed remains `used:false`. If no remaining proved
candidate excludes the current CTI, the original `unknown_cti` is returned.

The added invariant is sound because it was independently proved over the same
transition system before use. The target proof may assume it without changing
the set of reachable executions.

Ranked-`leadsTo` proof failures are not currently probed: their obligation
solvers have specialized state/model shapes. Candidates still receive their
independent verdict, but the retry loop stops if the remaining result is a
ranking failure.

## JSON contract

Every run with at least one `--lemma` adds:

```json
{
  "lemmas": [
    {
      "expression": "x == y",
      "name": "AuxiliaryLemma1",
      "status": "proved",
      "used": true,
      "proof": {
        "result": "proved",
        "k": 1,
        "checked_to_depth": 8,
        "completeness": "unbounded"
      }
    }
  ],
  "lemma_cti_exclusions": [
    {
      "lemma": "x == y",
      "target": "Sync",
      "k": 1,
      "violated_steps": [0, 1],
      "cti": {"states": [], "violated_at": 1}
    }
  ]
}
```

A rejected candidate has `status:"rejected"`, `used:false`, and its proof
result verbatim except for nondeterministic elapsed cost. A false lemma thus
retains `result:"violated"`, `violation_kind`, and `trace`; a non-inductive
lemma retains its `unknown_cti`; invalid syntax/types retain an `error` result.

When the target reaches `proved` using at least one lemma, the result also
contains:

```json
{
  "auxiliary_invariant_recommendation": {
    "message": "write the used proved lemmas into the specification as auxiliary invariants",
    "declarations": ["invariant AuxiliaryLemma1 { x == y }"]
  }
}
```

The command does not modify the source. Persisting a proved lemma remains an
explicit human/agent edit and review step.

## CLI, exit, and cache behavior

- `--lemma` is repeatable and valid only with `--engine induction`; using it
  with BMC is a usage error (exit 2).
- The final target result owns the process exit: `proved` is 0,
  `unknown_cti`/`violated` is 1, and an input/spec error is 2.
- Rejected candidates do not by themselves change the exit code if the target
  can still be proved with other candidates.
- Candidate strings and their order are part of the persistent verification
  cache key. A no-lemma result can never satisfy a lemma-guided request.

## Non-goals

- Automatic generation of candidate lemmas; existing CTI suggestions and
  agents provide candidates.
- Source rewriting.
- Unverified assumptions.
- General invariant synthesis, quantifier instantiation search, IC3, or PDR.
