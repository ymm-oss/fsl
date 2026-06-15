# Building the standalone binary

The mechanism for distributing `fslc` as a single executable file that does not
require Python. On release, `.github/workflows/release.yml` builds automatically
for each OS/arch and attaches the result to the GitHub Release (triggered by
pushing a `v*` tag).

## How it works

- Tool: PyInstaller's [`--onefile`](https://pyinstaller.org/)
- Bundling dependencies:
  - `--collect-all z3` … pulls in z3's native libz3 (`.dylib`/`.so`/`.dll`)
    (the only native dependency; with just this, even `verify` works with no
    external dependencies)
  - `--copy-metadata fslc` … makes `importlib.metadata.version("fslc")` resolvable
    even in a frozen environment (without it, `--version` falls back to `1.0.0`)
- Entry point: `packaging/fslc_entry.py` (just calls `fslc.cli.main`)

## Build and try locally

```bash
python3 -m venv /tmp/fsl-build && source /tmp/fsl-build/bin/activate
pip install . pyinstaller

pyinstaller --onefile --name fslc \
  --collect-all z3 \
  --copy-metadata fslc \
  packaging/fslc_entry.py

# Output: dist/fslc (dist/fslc.exe on Windows)
./dist/fslc --version
./dist/fslc verify examples/pm/cancel_flow.fsl
```

The generated binary is ~37MB (including z3). Because `--onefile` self-extracts at
startup, the first launch is slightly slow.

## Release procedure

```bash
git tag v1.1.0
git push origin v1.1.0
```

This kicks off the build for all platforms, and `fslc-<os>-<arch>` and `*.sha256`
are attached to the Release. When you just want to confirm it works locally, launch
it from the Actions tab with `workflow_dispatch` (in that case, the Release
attachment is skipped and it is kept as an artifact).

## Constraints / notes

- **No cross-building**: each OS/arch must be built on the corresponding runner
  (PyInstaller does not cross-compile). Handled by the matrix.
- **No macOS signing/notarization**: a downloaded executable is quarantined by
  Gatekeeper. On the user side, `xattr -d com.apple.quarantine <file>` is required.
  Proper notarization requires an Apple Developer certificate (currently out of
  scope).
- **macOS Intel (x86_64) is out of scope**: GitHub's `macos-13` (Intel) runners are
  exhausted and stay queued without being picked up, so it is removed from the
  matrix. The Apple Silicon migration is advancing and Intel Mac demand is
  shrinking. If it becomes necessary, just restore the
  `macos-13` / `target: macos-x64` / `asset: fslc-macos-x64` lines to the `build`
  matrix (z3 has an x86_64 wheel, so the build itself passes with the same recipe
  as the other Macs).
- **z3-solver is wheel-only** (`--only-binary=z3-solver`): prevents pip from falling
  back to a source build and failing when the latest version lacks a wheel for that
  OS/arch. pip automatically falls back to the most recent version that has a wheel.
