Documented (#787): `AGENTS.md`, `CONTRIBUTING.md`, `docs/DESIGN-conformance-harness.md`,
`docs/README.md`, `tests/test_dialect_conformance.py`, `tests/dialect_registry.py`,
`rust/fsl-syntax/src/causal.rs`, and `docs/DESIGN-causal.md` now state plainly that the
frozen-Python conformance harness and coupled-change parity checks (`tests/test_dialect_conformance.py`,
`tests/test_coupled_change_meta.py`) are developer-run manual/reference checks with no CI or
`tools/check-native-integration.sh` lane invoking them, instead of describing them as machine-enforced
gates. The underlying coupling rules (register a new dialect construct in `tests/dialect_registry.py`)
are unchanged; only the enforcement-mechanism claim was corrected. The `docs/LANGUAGE.ja.md` freshness
claim, which genuinely is a required CI check (`site-reference-freshness.yml`), is untouched.
