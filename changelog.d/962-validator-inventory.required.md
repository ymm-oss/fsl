Required (#962): add a machine-generated CI validator inventory and
reachability metatest so new `tests/test_*.py` modules fail closed unless they
are wired into a required gate or explicitly classified as exempt; slice 1
records the existing 98 unwired modules without wiring them.
