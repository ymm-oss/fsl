# Natural language → syntax

Use after the formalization memo is confirmed. A correspondence helps choose
syntax; it does not authorize a missing requirement. Unlisted/ambiguous formulas
remain questions for the human.

| Confirmed intent | Construct |
|---|---|
| Never reach a bad state / always true | kernel `invariant`; requirements named `always` rule |
| Constrain an old→new change | `trans`, with `old()` |
| X allowed only when Y | action `requires`; requirements `allow … only when` |
| Name a repeated predicate | file-local non-recursive `def name(p: T) = expr` |
| P must eventually become Q | `leadsTo` plus fair progress actions |
| P becomes Q within K action steps | `leadsTo Name { P ~> within K Q }` |
| Preserve P until Q, Q may never occur | `unless Name { P unless Q }` |
| Preserve P until Q and require Q | `until Name { P until Q }` |
| Business stage response | `policy ID "text" every Case in Source must eventually be Target` |
| Business completion possible | `goal ID "text" some Case can reach Target` / `all Case can be Target` |
| History: once X happened / cannot happen again | confirmed ghost + property; requirements `only after` when applicable |
| A state is possible | `reachable` witness (also catches over-constraint) |
| Relation between finite identities | `relation A -> B`; relation methods and graph predicates |
| Tick SLA / deadline | requirements `time` + rule `deadline`; [clock limits](impl.md#11-non-functional-requirements-nfr) |
| Numeric bounds | `type T = lo..hi`, or `number T` + verification values; no redundant boundary invariant |
| At most / less than / at least / greater than | `<=` / `<` / `>=` / `>`; confirm inclusivity explicitly |
| Aggregate conservation | invariant over `sum(...)` / `count(...)` |
| Case split decides who/which event | requirements `concept` + `decide`; source must settle gaps/overlaps |

Business has no `number`: keep numeric system detail in requirements/design.
For the full requirements phrases use its
[syntax owner](../../fsl-requirements/references/syntax.md), not kernel snippets.
