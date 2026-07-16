# FSL for VS Code

Language support for [FSL](https://github.com/ymm-oss/fsl) specification files (`.fsl`):

- Syntax highlighting (TextMate grammar — approximate, since most FSL keywords are contextual)
- Diagnostics (parse and type errors)
- Document outline (symbols)
- Go to definition, including `compose` `use ... from` cross-file resolution

## Requirements

This extension is a thin LSP client. It launches the **`fslc-lsp`** language server,
which must be available on your `PATH`. Install it with the project's `install.sh`
(it installs the native Rust server in `~/.local/bin`; Python is not required).

To point the extension at a specific server binary instead of `PATH`, set the
`FSLC_LSP_COMMAND` environment variable to that binary's path.

## Install

Download the `.vsix` from the [GitHub Releases](https://github.com/ymm-oss/fsl/releases)
page and install it:

```bash
code --install-extension fsl-vscode-<version>.vsix
```

or use **Extensions: Install from VSIX…** from the Command Palette.
