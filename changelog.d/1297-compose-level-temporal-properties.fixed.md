Fixed (#1297): a compose-level `leadsTo`, `until`, or `unless` can now read
component state through `alias.var`, as compose-level `invariant`, `trans`,
and `reachable` already could. Before, every such compose stopped with exit 2
(`public Kernel cannot type identifier 'a'`); it now gets a verdict. No
migration is needed beyond expecting a verdict (including `violated`, exit 1)
where the error used to be.
