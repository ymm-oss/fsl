Fixed (#1501): the generated language reference keeps wrapped line-start issue
references such as `#1448` and `#1200` as paragraph text instead of turning them
into headings and dropping the hash. Real space/tab-separated headings, fenced
code, and already escaped hashes retain their existing rendering.
