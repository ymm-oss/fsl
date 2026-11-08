Documented (#1440): the "Upgrading fslc from 4.12 to 5.0" guide in
`docs/manual/LANGUAGE.md` and `LANGUAGE.ja.md` corrects its 4.12 column:
plain `fslc migrate` already targeted `next` in 4.12 but planned nothing for a
headerless document, and 4.12 already refused some foreign options, such as
`explain --engine`, while it ignored others, such as `version --depth`. The
guide now says that most 4.12 ledger, HTML and scenarios approvals report
`drifted` with `rendering_changed` under 5.0 and need re-approval, and that
`renderer_changed` is not reported for a 4.x record. The FSL 4 to FSL 5
migration guidance says how to answer the `never_rule_without_forbidden`
warning a converted process form can add: add a `forbidden … then rejected`
trace for each guarded rule.
