# refine chaining mode — end-to-end fidelity in one command

Propagation through a layer chain (business ⊒ requirements ⊒ design …) had no way
to directly confirm **bottom ⊒ top** even when `refine` was run on each adjacent
pair individually; you had to implicitly trust transitivity (you could confirm it
by composing the mappings by hand, but that is easy to get wrong).

`fslc refine` can **check a chain by listing (spec, mapping) pairs**. It composes
the adjacent mappings (α_AC = α_BC ∘ α_AB, action correspondence a→b→c / stutter)
and checks bottom ⊒ top **directly**. Since bounded refinement is transitive at
the same depth, this is equivalent to and as sound as "all adjacent links hold"
(`docs/DESIGN-refinement.md` §7).

## Cast (3 layers: business ⊒ requirements ⊒ design)

| File | Layer | Detail added |
|---|---|---|
| `top.fsl` | Business (`ChainTop`) | Open → Done |
| `mid.fsl` | Requirements (`ChainMid`) | adds a review step `Review` |
| `bot.fsl` | Design (`ChainBot`) | adds an audit step `Audit` on top |
| `bot_refines_mid.fsl` | design ⊒ requirements | `audit` is a stutter in the requirements layer |
| `mid_refines_top.fsl` | requirements ⊒ business | `start_review` is a stutter in the business layer |

## Run

```bash
E=examples/refinement_chain

# Adjacent (one pair at a time, as before)
fslc refine $E/bot.fsl $E/mid.fsl $E/bot_refines_mid.fsl --depth 6   # refines
fslc refine $E/mid.fsl $E/top.fsl $E/mid_refines_top.fsl --depth 6   # refines

# Chain: listing (spec mapping) in sequence does a composed end-to-end check
fslc refine $E/bot.fsl \
            $E/mid.fsl $E/bot_refines_mid.fsl \
            $E/top.fsl $E/mid_refines_top.fsl --depth 6
```

Output of the chain check (success):

```json
{ "result": "refines", "impl": "ChainBot", "abs": "ChainTop",
  "action_map": { "start_review": "stutter", "audit": "stutter", "finish": "finish" },
  "chain": ["ChainBot", "ChainMid", "ChainTop"] }
```

`action_map` is composed (`audit`/`start_review` are stutters at the top, `finish`
corresponds to the top-layer `finish`). `chain` shows the order of the layers.

## Highlights

- **Direct end-to-end**: the behavior of the bottom layer `ChainBot` is mapped by
  the composed α into the vocabulary of the top layer `ChainTop`, and you can
  confirm in one command that it does not break the business contract.
- **Pinpointing a broken link**: if some adjacent mapping breaks fidelity, the
  result is `refinement_failed` plus `failed_link: {from, to, kind}` pointing to
  **the first link that broke** (see `tests/test_refinement_chain_example.py`).
- **Inside the composition**: both indexed maps (`st[c]`) and parameterized actions
  (`finish(c)`) are composed. It is unsupported only when an argument expression
  reads an intermediate layer's state (a type error to that effect).

Checked by: `tests/test_refinement_chain_example.py`.
