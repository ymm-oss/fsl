## Natural language → syntax mapping (from the formalization memo to the spec)

Map the sentences extracted during requirement normalization in the formalization
memo to syntax using the following correspondence. Whereas the idiom collection in
[syntax](syntax.md) goes "FSL → the correct way to write it," this is the reverse
lookup "natural language → which construct." **Free-form logical formulas not
covered by this table are easy to misread, so mark them for human confirmation in
the formalization memo.**

| Natural-language pattern | FSL construct |
|---|---|
| "must never" / "always the case" (prohibition, invariance) | `invariant` (safety) |
| "prohibit/constrain a change from one state to the next" (two-state safety) | `trans` (use `old()` to reference the pre-transition state) |
| "can only do X when Y" (precondition) | an action's `requires` |
| a long/repeated business condition needs a stable name | file-local non-recursive `def name(p: Type) = expr`, then call it from guards/properties |
| "once X happens, Y must eventually happen" (response, progress) | `leadsTo` + `fair` on the action that drives progress |
| "P must become Q within K steps" (bounded response) | `leadsTo Name { P ~> within K Q }` |
| "keep P true until Q" (safety, Q may never happen) | `unless Name { P unless Q }` |
| "keep P true until Q, and Q must happen" (safety + progress) | `until Name { P until Q }` |
| business-flow stage response for consultants/PMs | `policy POL-1 "..." every Case in Source must eventually be Target [or Target ...]` |
| business-flow reachability / completion goal | `goal G "..." some Case can reach Target` or `goal G "..." all Case can be Target [or Target ...]` |
| "once X has happened, it can never happen again" (history dependence) | ghost variable (`ever_*`) + invariant |
| "X can be reached / X can end up being reached" (possibility) | `reachable` (witness, or detection of over-constraint) |
| "A is linked to B" / graph reachability / acyclicity / functional relation | `state { r: relation A -> B }` plus `.contains/.add/.remove`, `reachable`, `acyclic`, `functional`, `injective`, `domain`, `range` |
| "within K times / K ticks" (deadline) | kernel `leadsTo ... within K` for step deadlines, or requirements `time` + `deadline` for SLA/tick semantics ([layers](layers.md)) |
| upper/lower bound or non-negativity of a number | kernel: `type T = lo..hi`; business/requirements dialects: `number T` plus `verify { values T = lo..hi }` (do not hand-write boundary invariants) |
| "at most / less than / at least / greater than" "before / after" | `<= / < / >= / >`. **Make boundary implications explicit in the memo** (the most frequent misreading) |
| "the total equals X" / "the count is X" (aggregate consistency) | an invariant over `sum(...)` / `count(...)` |
