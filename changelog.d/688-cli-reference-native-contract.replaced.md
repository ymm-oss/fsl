Replaced (#688): site CLI reference pages (`docs/intro/cli.{en,ja}.html`) now
render from the native `rust/fslc/cli-contract.json` contract instead of
frozen Python reference argparse introspection (`src/fslc/cli.py`), so the
public site stops describing the native CLI from a surface that is not its
authority. The move surfaces `fslc testplan`, `fslc counterexample`, and
`fslc mutate --oracle-attribution`, and drops no command: 29 of the
contract's 30 prog entries render on the page, `fslc version` staying
deliberately excluded as before.
