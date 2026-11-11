Added (#1815): in an FSL 5 `requirements` document, every `for` that names a
stage-qualified entity also reads it entity first, as `always` and `count` write
it: `decide ID "text" for Order in Placed { … }`, `allow … only for Order in
Placed`, `never … for Order in Packed`, alone or in an `and` (Japanese keywords:
`対象 Order 段階 Placed`). Each lowers to the same kernel and the same lowering
text as `for Placed Order`, and `fmt` keeps the order written (`fmt --lang`
translates `in` / `段階`). `Entity in Stage` names one stage, as `Stage Entity`
does: `for Order in Placed, Packed` is refused at the comma, naming the other
order. Inside a rule the `in` stands on the entity's line, the same newline rule
as `Stage Entity`; the stage after it may wrap, as a term after `and` may. `always … is`, decision-table rows, `count`, `concept` and
`policy` keep their existing orders. Both language references and the
fsl-requirements skill reference and keyword table state the form.
