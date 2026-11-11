Added (#1812): an `fsl 5` scenario can use its own instance names in two more
places. A relation role's value may name an instance of the related entity
(`User places o1 with buyer u1`, `購入者 利用者1 で`); such names are numbered from 0
per entity in order of first use together with the steps' instances, count
against `verify { instances }` (`scenario_instance_overflow`), and a name of
another entity's instance is refused; a number written as a role value takes
no slot (after `with buyer 0` the first User name is 0 too). In `then <expr>`,
`o1.qty` and `stage(o1)` / `Order.stage(o1)` read the lowered state at `o1`'s
number (`order_qty[0]`, `order_stage[0]`), in English and in Japanese; a
binder of the same name, or an `is some(o1)` pattern anywhere in the check,
keeps it, and a bare name is not read as an instance. `with buyer 0` and
`order_qty[0]` still work, and a name the document declares as a constant,
resource, enum member or stage keeps its meaning, so no document that checked
changes: `fslc check` of the 665 corpus `.fsl` files is byte-equal before and
after. The name form desugars to the number form, so `scenarios`, `testgen`,
`explain`, `kernel` and `document` print the same output for both (instance
names were never printed). The missing-role message now says `with <role>
<instance>` instead of `with <role> <number>`. Documented in both language
references, `skills/fsl-requirements/references/syntax.md` (replacing #1825's
"its name cannot be written there") and `keywords.md`,
`skills/fsl/references/syntax.md`, and `docs/design/DESIGN-dialects.md`.
