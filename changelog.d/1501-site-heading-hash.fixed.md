Fixed (#1501): the generated language reference keeps wrapped line-start issue
references such as `#1448` and `#1200` as paragraph text instead of turning them
into headings and dropping the hash. Only the heading separator rule changes:
one to six hashes require a space, tab, or end of line. The installed
Python-Markdown version's heading-body and closing-hash behavior, including
escaped hashes, is preserved; fenced code and already escaped paragraph hashes
are unchanged. Lines beginning with seven or more hashes now remain prose rather
than becoming h6 headings, matching CommonMark and GitHub.
