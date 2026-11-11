Added (#1803): an `fsl 5` requirements event may write its move in parentheses after the phrase
and its inputs, `event reject = Manager rejects Claim (Submitted -> Rejected)` /
`出来事 reject = 上長 が 申請 を 却下する (提出済み -> 却下済み)`, so the sentence ends before the move.
The bare `Submitted -> Rejected` stays accepted, and both forms lower to the same kernel. `fmt`
keeps the form written, with one space before `(`. The parentheses hold one move: a stay keeps
`in S` / `段階 S` without them, so `(in S)` and `(S)` are parse errors, and full-width `（ ）` is
`fullwidth_symbol`. The language references and the `fsl-requirements` skill now show the
parenthesized form.
