Fixed (#1197): a compose component's `until` / `unless` is now rewritten like
its other properties: the name is reported as `alias.name` and its operands
read the component's own state. Before, a compose using such a component
failed with exit 2 (`cannot type identifier`), two components declaring the
same `until` name collided (`duplicate property name`), and when the compose
declared state with the same name as the component's, the property silently
checked the compose's variable instead. Migration: a compose in that last
shape that reported `verified` can now report `violated` (exit 1) for a
component property that does not hold; the `until` / `unless` result keys
change from `U` to `alias.U`.
