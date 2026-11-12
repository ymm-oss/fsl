Fixed (#1839): `fslc explain --readable` no longer runs the bounded verifier,
the witness search or the counterfactual analysis, none of which its text
prints, so its cost no longer grows with `--depth`. The readable text is
unchanged byte for byte; a unit test fails if `--readable` runs the bounded
verifier.
