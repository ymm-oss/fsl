Fixed (#1281): a mutant's killer no longer depends on earlier solver queries in
the same run; each mutation BMC call uses a fresh thread-local Z3 context.
