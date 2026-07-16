#!/usr/bin/env bash
set -euo pipefail

INSTALL_SKILL=1
CLONE_URL="https://github.com/ymm-oss/fsl.git"
INSTALL_DIR="${FSL_INSTALL_DIR:-$HOME/.fsl}"

fail() {
  echo "Error: $*" >&2
  exit 1
}

usage() {
  echo "Usage: bash install.sh [--no-skill]"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --no-skill)
      INSTALL_SKILL=0
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "Unknown option: $1. The only available option is --no-skill."
      ;;
  esac
  shift
done

is_fsl_repo() {
  repo="$1"
  [ -f "$repo/pyproject.toml" ] || return 1
  [ -d "$repo/src/fslc" ] || return 1
  [ -f "$repo/specs/cart_v1.fsl" ] || return 1
  grep -q 'name = "fslc"' "$repo/pyproject.toml" 2>/dev/null
}

find_repo_upwards() {
  start="$1"
  dir=$(cd "$start" 2>/dev/null && pwd -P) || return 1
  while :; do
    if is_fsl_repo "$dir"; then
      printf '%s\n' "$dir"
      return 0
    fi
    [ "$dir" = "/" ] && return 1
    dir=$(dirname "$dir")
  done
}

REPO_DIR=""
if REPO_DIR=$(find_repo_upwards "$PWD"); then
  :
else
  SCRIPT_SOURCE="${BASH_SOURCE[0]:-}"
  if [ -n "$SCRIPT_SOURCE" ] && [ -f "$SCRIPT_SOURCE" ]; then
    SCRIPT_DIR=$(cd "$(dirname "$SCRIPT_SOURCE")" && pwd -P)
    if REPO_DIR=$(find_repo_upwards "$SCRIPT_DIR"); then
      :
    fi
  fi
fi

if [ -z "$REPO_DIR" ]; then
  if ! command -v git >/dev/null 2>&1; then
    fail "git is required. Install it from https://git-scm.com/, or fetch the repository from GitHub and run ./install.sh."
  fi

  if [ -e "$INSTALL_DIR" ]; then
    [ -d "$INSTALL_DIR/.git" ] || fail "$INSTALL_DIR already exists but is not a Git repository. Remove or move it and re-run."
    echo "Updating the FSL repository: $INSTALL_DIR"
    git -C "$INSTALL_DIR" pull --ff-only || fail "Failed to update $INSTALL_DIR. Check for local changes, or remove it and re-run."
  else
    echo "Fetching the FSL repository: $INSTALL_DIR"
    # Public repository. Use the gh CLI if available; otherwise clone over https (no authentication required).
    if command -v gh >/dev/null 2>&1; then
      gh repo clone ymm-oss/fsl "$INSTALL_DIR" || fail "Failed to fetch the repository. Check your network connection."
    else
      git clone "$CLONE_URL" "$INSTALL_DIR" || fail "Failed to fetch the repository. Check your network connection (no authentication is required since this is a public repository)."
    fi
  fi
  REPO_DIR=$(cd "$INSTALL_DIR" && pwd -P)
else
  REPO_DIR=$(cd "$REPO_DIR" && pwd -P)
  if [ -d "$REPO_DIR/.git" ]; then
    # Developer working tree (git checkout): use it in place
    echo "Using this repository: $REPO_DIR"
  elif [ "$REPO_DIR" = "$INSTALL_DIR" ]; then
    # Already placed at $INSTALL_DIR (re-run)
    echo "Using the already-installed folder: $REPO_DIR"
  else
    case "$REPO_DIR/" in
      "$INSTALL_DIR"/*)
        # If somehow run from within $INSTALL_DIR, use it in place
        echo "Using this folder: $REPO_DIR"
        ;;
      *)
        # Source extracted from a ZIP, etc.: place it at a stable location ($INSTALL_DIR) before using it
        echo "Placing the downloaded folder at $INSTALL_DIR."
        SRC_DIR="$REPO_DIR"
        mkdir -p "$INSTALL_DIR"
        # Keep .venv (preserve the environment on re-run); replace the sources
        find "$INSTALL_DIR" -mindepth 1 -maxdepth 1 ! -name .venv -exec rm -rf {} + 2>/dev/null || true
        (
          cd "$SRC_DIR" && for item in * .[!.]*; do
            case "$item" in .venv|.git) continue ;; esac
            [ -e "$item" ] || continue
            cp -R "$item" "$INSTALL_DIR/"
          done
        ) || fail "Failed to place files at $INSTALL_DIR. Remove $INSTALL_DIR and re-run."
        REPO_DIR=$(cd "$INSTALL_DIR" && pwd -P)
        is_fsl_repo "$REPO_DIR" || fail "Failed to place files at $INSTALL_DIR. Remove $INSTALL_DIR and re-run."
        echo "Placed at: ${REPO_DIR} (you may delete the downloaded folder)"
        ;;
    esac
  fi
fi

NATIVE_DIR="$REPO_DIR/.native/bin"
FSL_BIN="$NATIVE_DIR/fslc"
FSL_LSP_BIN="$NATIVE_DIR/fslc-lsp"
mkdir -p "$NATIVE_DIR"

native_target() {
  os=$(uname -s)
  arch=$(uname -m)
  case "$os:$arch" in
    Darwin:arm64|Darwin:aarch64) echo "macos-arm64" ;;
    Darwin:x86_64|Darwin:amd64) echo "macos-x64" ;;
    Linux:x86_64|Linux:amd64) echo "linux-x64" ;;
    Linux:aarch64|Linux:arm64) echo "linux-arm64" ;;
    *) fail "No native FSL release is available for $os/$arch." ;;
  esac
}

download_file() {
  url="$1"
  destination="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$destination"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$destination"
  else
    fail "curl or wget is required to download the native FSL binaries."
  fi
}

RELEASE_URL="https://github.com/ymm-oss/fsl/releases/latest/download"
install_release_asset() {
  asset="$1"
  destination="$2"
  echo "Installing native Rust binary: $asset"
  download_file "$RELEASE_URL/$asset" "$destination.download" || fail "Failed to download $asset from the latest GitHub Release."
  download_file "$RELEASE_URL/$asset.sha256" "$destination.sha256.download" || fail "Failed to download the checksum for $asset."
  expected_hash=$(awk '{print $1}' "$destination.sha256.download")
  if command -v shasum >/dev/null 2>&1; then
    actual_hash=$(shasum -a 256 "$destination.download" | awk '{print $1}')
  elif command -v sha256sum >/dev/null 2>&1; then
    actual_hash=$(sha256sum "$destination.download" | awk '{print $1}')
  else
    fail "shasum or sha256sum is required to verify the native binaries."
  fi
  [ "$expected_hash" = "$actual_hash" ] || fail "Checksum verification failed for $asset."
  mv "$destination.download" "$destination"
  rm -f "$destination.sha256.download"
  chmod +x "$destination"
}

TARGET=$(native_target)
install_release_asset "fslc-$TARGET" "$FSL_BIN"
install_release_asset "fslc-lsp-$TARGET" "$FSL_LSP_BIN"

LOCAL_BIN="$HOME/.local/bin"
mkdir -p "$LOCAL_BIN"

link_command() {
  cmd_name="$1"
  target="$2"
  link_path="$LOCAL_BIN/$cmd_name"
  if [ -L "$link_path" ]; then
    link_target=$(readlink "$link_path" || true)
    if [ "$link_target" = "$target" ]; then
      echo "The $cmd_name command link is already set up: $link_path"
    elif [ "$link_target" = "$REPO_DIR/.venv/bin/$cmd_name" ]; then
      ln -sfn "$target" "$link_path"
      echo "Migrated the $cmd_name command link from Python to the native binary: $link_path"
    else
      echo "Warning: $link_path points to a different location (${link_target}). Not overwriting."
    fi
  elif [ -e "$link_path" ]; then
    echo "Warning: $link_path already exists. Not overwriting. If needed, remove it manually and re-run."
  else
    ln -s "$target" "$link_path"
    echo "Created the $cmd_name command link: $link_path"
  fi
}

link_command fslc "$FSL_BIN"
# The VS Code extension launches `fslc-lsp` from PATH.
link_command fslc-lsp "$FSL_LSP_BIN"

case ":$PATH:" in
  *":$LOCAL_BIN:"*)
    ;;
  *)
    SHELL_NAME=$(basename "${SHELL:-}")
    if [ "$SHELL_NAME" = "zsh" ]; then
      echo "$LOCAL_BIN is not on your PATH. If needed, add this line to ~/.zshrc: export PATH=\"\$HOME/.local/bin:\$PATH\""
    elif [ "$SHELL_NAME" = "bash" ]; then
      echo "$LOCAL_BIN is not on your PATH. If needed, add this line to ~/.bashrc: export PATH=\"\$HOME/.local/bin:\$PATH\""
    else
      echo "$LOCAL_BIN is not on your PATH. If needed, add this line to your shell config file: export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
    ;;
esac

if [ "$INSTALL_SKILL" -eq 1 ]; then
  SKILL_NAMES="fsl fsl-business fsl-requirements fsl-design fsl-design-review fsl-delivery"
  mkdir -p "$HOME/.claude/skills"
  for SKILL_NAME in $SKILL_NAMES; do
    SKILL_SRC="$REPO_DIR/skills/$SKILL_NAME"
    SKILL_DST="$HOME/.claude/skills/$SKILL_NAME"
    [ -d "$SKILL_SRC" ] || fail "$SKILL_SRC not found. Check that the repository was fetched correctly and re-run."
    if [ -e "$SKILL_DST" ] || [ -L "$SKILL_DST" ]; then
      echo "The Claude Code skill already exists: $SKILL_DST — to update this skill, run rm -rf \"$SKILL_DST\" and re-run."
    else
      cp -R "$SKILL_SRC" "$SKILL_DST"
      echo "Copied Claude Code skill: $SKILL_DST"
    fi
  done
else
  echo "Skipped copying the Claude Code skills."
fi

echo "Running a smoke test."
CHECK_OUTPUT=$("$FSL_BIN" check "$REPO_DIR/specs/cart_v1.fsl") || fail "Smoke test failed. Check the result of $FSL_BIN check $REPO_DIR/specs/cart_v1.fsl."
case "$CHECK_OUTPUT" in
  *'"result": "ok"'*|*'"result":"ok"'*)
    echo "Smoke test ok: fslc check specs/cart_v1.fsl"
    ;;
  *)
    fail "Smoke test was not ok. Check the output of $FSL_BIN check $REPO_DIR/specs/cart_v1.fsl."
    ;;
esac

echo "Done. Open Claude Code and try saying: \"Use \$fsl-requirements to formalize and verify these cancellation requirements.\" Examples: $REPO_DIR/examples/pm/ (PMs), $REPO_DIR/examples/consulting/ (consultants)"
