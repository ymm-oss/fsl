{
  description = "FSL — AI-native formal specification language verifier (fslc)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    # nixpkgs-unstable (26.11) has dropped x86_64-darwin support.
    # Pin x86_64-darwin to the 26.05 stable branch, which still supports
    # it and is compatible with crane (requires nixpkgs >= 26.05).
    nixpkgs-darwin.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
    flake-utils.url = "github:numtide/flake-utils";
    # crane uses `cargo vendor` (cargo's own HTTP client with a proper
    # User-Agent) rather than Nix fetchurl, which avoids crates.io 403
    # rejections when downloading crate tarballs.
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, nixpkgs-darwin, flake-utils, crane, ... }@inputs:
    (flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs =
          if system == "x86_64-darwin"
          then nixpkgs-darwin.legacyPackages.${system}
          else nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        fslc = craneLib.buildPackage {
          pname = "fslc";
          version = "4.4.1";
          # The Cargo workspace lives under rust/; build from there so
          # crane finds Cargo.toml and Cargo.lock at the source root.
          # Use cleanSource (not crane's cleanCargoSource) because
          # fsl-tools uses include_str! for non-Rust files (.css, .txt)
          # that cleanCargoSource would filter out.
          src = pkgs.lib.cleanSource ./rust;
          # Build only the native CLI and LSP server; fsl-wasm targets
          # wasm32-unknown-unknown and is not buildable on native targets.
          cargoBuild = "cargo build --release -p fslc-rust -p fsl-lsp";
          # The project's own CI runs the full test suite; keep the Nix
          # build focused on producing installable binaries.
          doCheck = false;
          # The z3 crate uses the "vendored" feature, which builds Z3 from
          # bundled source via cmake.
          nativeBuildInputs = [ pkgs.cmake pkgs.pkg-config pkgs.python3 ];
          buildInputs =
            pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
          meta = {
            description = "AI-native formal specification language verifier with bundled Z3";
            homepage = "https://github.com/ymm-oss/fsl";
            license = pkgs.lib.licenses.asl20;
            mainProgram = "fslc";
          };
        };
      in
      {
        packages = {
          # Users naturally try .#fslc, so expose it alongside default.
          fslc = fslc;
          default = fslc;
          source = fslc;
        };

        apps = {
          fslc = {
            type = "app";
            program = "${fslc}/bin/fslc";
          };
          default = {
            type = "app";
            program = "${fslc}/bin/fslc";
          };
        };

        checks = {
          build = fslc;
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.cmake pkgs.pkg-config pkgs.python3 ];
          buildInputs =
            [ pkgs.rustc pkgs.cargo ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
        };
      }
    )) // {
      overlays.default = final: prev: {
        fslc = self.packages.${final.system}.fslc;
      };
    };
}
