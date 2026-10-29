Breaking removal (#1602, #1603): `check` now rejects `__` in goal and policy IDs,
requirement IDs (`rule` in the next edition), scenario IDs (`acceptance` in
legacy), forbidden IDs, decision-table IDs (`decide`), and glossary aliases,
in both editions wherever the construct exists. These previously accepted names
now produce exit 2, `kind: "name"`, and structured `loc`, using the existing
reserved-separator message. Next-edition rule IDs use only their exact header
token spans, even with deadlines or preceding comments. Explicit `requirement`
blocks are supported and their IDs checked in both editions,
with or without deadlines; only rule-generated copies lose the duplicate ID check.
Known legacy-syntax limitation (also for explicit blocks):
split-spelled requirement, acceptance and forbidden IDs such as `REQ - __1` may
be located at the declaration keyword or a later repeated normalized spelling
rather than the ID tokens; rejection is unchanged and that limitation is not
fixed here. Rename IDs and aliases without `__` and update their references;
references to generated names remain allowed.
