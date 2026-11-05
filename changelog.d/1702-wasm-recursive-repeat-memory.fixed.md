Fixed: the WASM browser test's #1467 recursive repeat comparisons no longer
compare Z3's ambient allocator footprint (`cost.solver.memory_mb`), which
differed by 0.01 MB between identical native runs and failed the merge queue
with `parity_violation`. Each repeat packet must still report a present, finite,
positive value; every other field, including conflicts, decisions and
propagations, stays exact, and the cross-runtime comparison is unchanged. No
product behavior changes.
