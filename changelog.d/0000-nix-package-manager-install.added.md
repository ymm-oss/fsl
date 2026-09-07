<!-- SPDX-License-Identifier: Apache-2.0 -->
Nix flake support for installing `fslc` and `fslc-lsp` from source via
`nix build .#fslc`, `nix run .#default -- --version`, or
`nix profile install github:ymm-oss/fsl`. The flake uses crane for crate
vendoring, builds vendored Z3 via CMake, and supports `x86_64-linux`,
`aarch64-linux`, `x86_64-darwin`, and `aarch64-darwin`. A Devbox
configuration and Nix CI workflow are included.
