Changed (#1518): Breaking removal: check now rejects reads in init forall bodies
or filters of roots written by the same forall, except locations already written
at the current binder key in the current iteration. This root-based removal
includes correct forms with agreeing base-engine results, as well as
prior-forall other-key and nested-forall reads already refused by explicit and
Monitor with `before it is assigned` and
`nested forall in init is not supported`, respectively.
Fixed targets in empty or not statically provable nonempty domains no longer
count as assigned, preventing explicit and Monitor from accepting defaults.
Even after `flag = true`, a fixed-target write under `where flag and k == 0`
is conservatively refused with `init does not assign` despite a nonempty runtime
domain, not given an incorrect verdict. The 597-form corpus sweep found 0
ok-to-error transitions.
