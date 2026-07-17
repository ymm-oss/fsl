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

latest_release_tag() {
  local api metadata tag
  api="https://api.github.com/repos/ymm-oss/fsl/releases/latest"
  if command -v curl >/dev/null 2>&1; then
    metadata=$(curl -fsSL "$api")
  elif command -v wget >/dev/null 2>&1; then
    metadata=$(wget -qO- "$api")
  else
    fail "curl or wget is required to resolve the latest FSL release."
  fi
  tag=$(printf '%s\n' "$metadata" | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p')
  [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "The latest GitHub Release has an invalid tag: $tag"
  printf '%s\n' "$tag"
}

command -v git >/dev/null 2>&1 || fail "git is required. Install it from https://git-scm.com/."
RELEASE_TAG=$(latest_release_tag)

if [ -e "$INSTALL_DIR" ]; then
  [ -d "$INSTALL_DIR/.git" ] || fail "$INSTALL_DIR already exists but is not a Git repository. Remove or move it and re-run."
  unexpected=$(git -C "$INSTALL_DIR" status --porcelain --untracked-files=all | awk '$0 !~ /^\?\? \.native\//')
  [ -z "$unexpected" ] || fail "$INSTALL_DIR has local changes. Move them or remove the directory and re-run."
  echo "Updating the FSL repository to $RELEASE_TAG: $INSTALL_DIR"
  git -C "$INSTALL_DIR" fetch --force --depth 1 origin "refs/tags/$RELEASE_TAG:refs/tags/$RELEASE_TAG" || fail "Failed to fetch $RELEASE_TAG."
  git -C "$INSTALL_DIR" checkout --detach "$RELEASE_TAG" || fail "Failed to check out $RELEASE_TAG."
else
  echo "Fetching FSL $RELEASE_TAG: $INSTALL_DIR"
  git clone --branch "$RELEASE_TAG" --depth 1 "$CLONE_URL" "$INSTALL_DIR" || fail "Failed to fetch $RELEASE_TAG. Check your network connection."
fi
REPO_DIR=$(cd "$INSTALL_DIR" && pwd -P)

NATIVE_DIR="$REPO_DIR/.native/bin"
FSL_BIN="$NATIVE_DIR/fslc"
FSL_LSP_BIN="$NATIVE_DIR/fslc-lsp"
mkdir -p "$NATIVE_DIR"

native_target() {
  local os arch
  os=$(uname -s)
  arch=$(uname -m)
  case "$os:$arch" in
    Darwin:arm64|Darwin:aarch64) echo "macos-arm64" ;;
    Linux:x86_64|Linux:amd64) echo "linux-x64" ;;
    Linux:aarch64|Linux:arm64) echo "linux-arm64" ;;
    *) fail "No native FSL release is available for $os/$arch." ;;
  esac
}

download_file() {
  local url="$1"
  local destination="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$destination"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$destination"
  else
    fail "curl or wget is required to download the native FSL binaries."
  fi
}

RELEASE_URL="https://github.com/ymm-oss/fsl/releases/download/$RELEASE_TAG"
stage_release_asset() {
  local asset="$1"
  local destination="$2"
  local expected_hash actual_hash
  echo "Installing native Rust binary: $asset"
  download_file "$RELEASE_URL/$asset" "$destination.download" || fail "Failed to download $asset from $RELEASE_TAG."
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
  rm -f "$destination.sha256.download"
  chmod +x "$destination.download"
}

TARGET=$(native_target)
STAGING_DIR=$(mktemp -d "$NATIVE_DIR/.install.XXXXXX")
trap 'rm -rf "$STAGING_DIR"' EXIT
stage_release_asset "fslc-$TARGET" "$STAGING_DIR/fslc"
stage_release_asset "fslc-lsp-$TARGET" "$STAGING_DIR/fslc-lsp"

mv "$STAGING_DIR/fslc.download" "$FSL_BIN"
mv "$STAGING_DIR/fslc-lsp.download" "$FSL_LSP_BIN"
rm -rf "$STAGING_DIR"
trap - EXIT

LOCAL_BIN="$HOME/.local/bin"
mkdir -p "$LOCAL_BIN"

link_command() {
  local cmd_name="$1"
  local target="$2"
  local link_path="$LOCAL_BIN/$cmd_name"
  local link_target
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
    if [ -L "$SKILL_DST" ] && [ "$(readlink "$SKILL_DST")" = "$SKILL_SRC" ]; then
      echo "The Claude Code skill link is current: $SKILL_DST"
    elif [ -e "$SKILL_DST" ] || [ -L "$SKILL_DST" ]; then
      SKILL_BACKUP="$SKILL_DST.pre-native-v3"
      [ ! -e "$SKILL_BACKUP" ] && [ ! -L "$SKILL_BACKUP" ] || fail "$SKILL_BACKUP already exists. Move it and re-run."
      mv "$SKILL_DST" "$SKILL_BACKUP"
      ln -s "$SKILL_SRC" "$SKILL_DST"
      echo "Linked Claude Code skill: $SKILL_DST (previous copy preserved at $SKILL_BACKUP)"
    else
      ln -s "$SKILL_SRC" "$SKILL_DST"
      echo "Linked Claude Code skill: $SKILL_DST"
    fi
  done
else
  echo "Skipped linking the Claude Code skills."
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
