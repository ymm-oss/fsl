#!/usr/bin/env bash
set -euo pipefail

INSTALL_SKILL=1
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
if [ -n "${FSL_DATA_DIR:-}" ]; then
  INSTALL_DIR="$FSL_DATA_DIR"
elif [ -n "${FSL_INSTALL_DIR:-}" ]; then
  INSTALL_DIR="$FSL_INSTALL_DIR"
  echo "Warning: FSL_INSTALL_DIR is deprecated; use FSL_DATA_DIR instead."
else
  INSTALL_DIR="$DATA_HOME/fsl"
fi
case "$INSTALL_DIR" in
  /*) ;;
  *) fail_message="FSL_DATA_DIR must be an absolute path: $INSTALL_DIR" ;;
esac

fail() {
  echo "Error: $*" >&2
  exit 1
}

[ -z "${fail_message:-}" ] || fail "$fail_message"

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

download_file() {
  local url="$1"
  local destination="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$destination"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$destination"
  else
    fail "curl or wget is required to install FSL."
  fi
}

latest_release_tag() {
  local api metadata tag
  api="https://api.github.com/repos/ymm-oss/fsl/releases/latest"
  metadata=$(mktemp)
  download_file "$api" "$metadata" || fail "Failed to resolve the latest FSL release."
  tag=$(sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' "$metadata")
  rm -f "$metadata"
  [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "The latest GitHub Release has an invalid tag: $tag"
  printf '%s\n' "$tag"
}

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

hash_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    fail "shasum or sha256sum is required to verify FSL release assets."
  fi
}

stage_release_asset() {
  local asset="$1"
  local destination="$2"
  local expected_hash actual_hash
  echo "Downloading release asset: $asset"
  download_file "$RELEASE_URL/$asset" "$destination.download" || return 1
  download_file "$RELEASE_URL/$asset.sha256" "$destination.sha256.download" || return 1
  expected_hash=$(awk '{print $1}' "$destination.sha256.download")
  actual_hash=$(hash_file "$destination.download")
  [ "$expected_hash" = "$actual_hash" ] || fail "Checksum verification failed for $asset."
  rm -f "$destination.sha256.download"
}

stage_skills() {
  local archive source_root skill_name
  archive="$STAGING_DIR/fsl-skills.tar.gz"
  mkdir -p "$STAGING_DIR/skills"
  if [ "$RELEASE_TAG" != "v3.0.0" ]; then
    stage_release_asset "fsl-skills.tar.gz" "$archive" \
      || fail "Failed to download the checksummed skill bundle from $RELEASE_TAG."
    tar -xzf "$archive.download" -C "$STAGING_DIR"
    rm -f "$archive.download"
    return
  fi

  # v3.0.0 predates the checksummed skill bundle. Keep compatibility by
  # extracting only the skills from the immutable release tag archive.
  rm -f "$archive.download" "$archive.sha256.download"
  echo "The release has no skill bundle; extracting skills from source tag $RELEASE_TAG."
  archive="$STAGING_DIR/source.tar.gz"
  download_file "https://github.com/ymm-oss/fsl/archive/refs/tags/$RELEASE_TAG.tar.gz" "$archive" \
    || fail "Failed to download the source archive for $RELEASE_TAG."
  [ "$(hash_file "$archive")" = "d2d691a98af28f4aaa77ded08b35978539a0d1e3c65e8b7f29783f143a447598" ] \
    || fail "Checksum verification failed for the v3.0.0 source archive."
  mkdir -p "$STAGING_DIR/source"
  tar -xzf "$archive" -C "$STAGING_DIR/source"
  source_root=$(find "$STAGING_DIR/source" -mindepth 1 -maxdepth 1 -type d | head -n 1)
  [ -n "$source_root" ] || fail "The source archive for $RELEASE_TAG is empty."
  for skill_name in fsl fsl-business fsl-requirements fsl-design fsl-design-review fsl-delivery; do
    [ -d "$source_root/skills/$skill_name" ] || fail "The $skill_name skill is missing from $RELEASE_TAG."
    cp -R "$source_root/skills/$skill_name" "$STAGING_DIR/skills/$skill_name"
  done
  rm -rf "$STAGING_DIR/source" "$archive"
}

RELEASE_TAG=$(latest_release_tag)
RELEASE_URL="https://github.com/ymm-oss/fsl/releases/download/$RELEASE_TAG"
TARGET=$(native_target)
RELEASES_DIR="$INSTALL_DIR/releases"
mkdir -p "$RELEASES_DIR"
STAGING_DIR=$(mktemp -d "$RELEASES_DIR/.install.XXXXXX")
trap 'rm -rf "$STAGING_DIR"' EXIT
mkdir -p "$STAGING_DIR/bin"

# Download and verify the complete native pair before changing the active install.
stage_release_asset "fslc-$TARGET" "$STAGING_DIR/bin/fslc" \
  || fail "Failed to download fslc-$TARGET from $RELEASE_TAG."
stage_release_asset "fslc-lsp-$TARGET" "$STAGING_DIR/bin/fslc-lsp" \
  || fail "Failed to download fslc-lsp-$TARGET from $RELEASE_TAG."
mv "$STAGING_DIR/bin/fslc.download" "$STAGING_DIR/bin/fslc"
mv "$STAGING_DIR/bin/fslc-lsp.download" "$STAGING_DIR/bin/fslc-lsp"
chmod +x "$STAGING_DIR/bin/fslc" "$STAGING_DIR/bin/fslc-lsp"
stage_skills

EXPECTED_VERSION="fslc ${RELEASE_TAG#v}"
ACTUAL_VERSION=$("$STAGING_DIR/bin/fslc" --version) \
  || fail "The downloaded native fslc did not start."
[ "$ACTUAL_VERSION" = "$EXPECTED_VERSION" ] \
  || fail "The downloaded binary reports '$ACTUAL_VERSION'; expected '$EXPECTED_VERSION'."

CLI_HASH=$(hash_file "$STAGING_DIR/bin/fslc")
RELEASE_NAME="$RELEASE_TAG-${CLI_HASH:0:12}"
RELEASE_DIR="$RELEASES_DIR/$RELEASE_NAME"
if [ -e "$RELEASE_DIR" ]; then
  [ -x "$RELEASE_DIR/bin/fslc" ] && [ -x "$RELEASE_DIR/bin/fslc-lsp" ] \
    || fail "$RELEASE_DIR does not contain an executable native pair."
  diff -qr "$STAGING_DIR" "$RELEASE_DIR" >/dev/null \
    || fail "$RELEASE_DIR differs from the verified $RELEASE_TAG payload. Move it and re-run."
  rm -rf "$STAGING_DIR"
else
  mv "$STAGING_DIR" "$RELEASE_DIR"
fi
trap - EXIT

CURRENT_LINK="$INSTALL_DIR/current"
if [ -e "$CURRENT_LINK" ] && [ ! -L "$CURRENT_LINK" ]; then
  fail "$CURRENT_LINK exists and is not a symbolic link. Move it and re-run."
fi
FSL_BIN="$CURRENT_LINK/bin/fslc"
FSL_LSP_BIN="$CURRENT_LINK/bin/fslc-lsp"

LOCAL_BIN="${FSL_BIN_DIR:-$HOME/.local/bin}"
case "$LOCAL_BIN" in
  /*) ;;
  *) fail "FSL_BIN_DIR must be an absolute path: $LOCAL_BIN" ;;
esac
mkdir -p "$LOCAL_BIN"

preflight_command_link() {
  local cmd_name="$1"
  local target="$2"
  local link_path="$LOCAL_BIN/$cmd_name"
  local link_target
  if [ -L "$link_path" ]; then
    link_target=$(readlink "$link_path" || true)
    case "$link_target" in
      "$target"|"$HOME/.fsl/.venv/bin/$cmd_name"|"$HOME/.fsl/.native/bin/$cmd_name") return ;;
    esac
    [ ! -e "$link_path" ] \
      || fail "$link_path points to a different installation ($link_target). Move it and re-run."
  elif [ -e "$link_path" ]; then
    fail "$link_path already exists and is not managed by FSL. Move it and re-run."
  fi
}

preflight_skill_link() {
  local skill_name="$1"
  local source="$CURRENT_LINK/skills/$skill_name"
  local destination="$HOME/.claude/skills/$skill_name"
  local backup="$destination.pre-native-v3"
  [ -d "$RELEASE_DIR/skills/$skill_name" ] \
    || fail "$RELEASE_DIR is missing the $skill_name skill."
  if [ -L "$destination" ] && [ "$(readlink "$destination")" = "$source" ]; then
    return
  fi
  if [ -e "$destination" ] || [ -L "$destination" ]; then
    [ ! -e "$backup" ] && [ ! -L "$backup" ] \
      || fail "$backup already exists. Move it and re-run."
  fi
}

preflight_command_link fslc "$FSL_BIN"
preflight_command_link fslc-lsp "$FSL_LSP_BIN"
if [ "$INSTALL_SKILL" -eq 1 ]; then
  for SKILL_NAME in fsl fsl-business fsl-requirements fsl-design fsl-design-review fsl-delivery; do
    preflight_skill_link "$SKILL_NAME"
  done
fi

link_command() {
  local cmd_name="$1"
  local target="$2"
  local link_path="$LOCAL_BIN/$cmd_name"
  local link_target
  if [ -L "$link_path" ]; then
    link_target=$(readlink "$link_path" || true)
    if [ "$link_target" = "$target" ]; then
      echo "The $cmd_name command link is current: $link_path"
      return
    fi
    case "$link_target" in
      "$HOME/.fsl/.venv/bin/$cmd_name"|"$HOME/.fsl/.native/bin/$cmd_name")
        ln -sfn "$target" "$link_path"
        echo "Migrated the $cmd_name command link to the native $RELEASE_TAG binary: $link_path"
        return
        ;;
    esac
    [ -e "$link_path" ] || {
      ln -sfn "$target" "$link_path"
      echo "Replaced a broken $cmd_name command link: $link_path"
      return
    }
    fail "$link_path points to a different installation ($link_target). Move it and re-run."
  elif [ -e "$link_path" ]; then
    fail "$link_path already exists and is not managed by FSL. Move it and re-run."
  else
    ln -s "$target" "$link_path"
    echo "Created the $cmd_name command link: $link_path"
  fi
}

link_command fslc "$FSL_BIN"
link_command fslc-lsp "$FSL_LSP_BIN"

case ":$PATH:" in
  *":$LOCAL_BIN:"*) ;;
  *)
    SHELL_NAME=$(basename "${SHELL:-}")
    if [ "$SHELL_NAME" = "zsh" ]; then
      echo "$LOCAL_BIN is not on your PATH. Add this line to ~/.zshrc: export PATH=\"\$HOME/.local/bin:\$PATH\""
    elif [ "$SHELL_NAME" = "bash" ]; then
      echo "$LOCAL_BIN is not on your PATH. Add this line to ~/.bashrc: export PATH=\"\$HOME/.local/bin:\$PATH\""
    else
      echo "$LOCAL_BIN is not on your PATH. Add it in your shell configuration."
    fi
    ;;
esac

if [ "$INSTALL_SKILL" -eq 1 ]; then
  mkdir -p "$HOME/.claude/skills"
  for SKILL_NAME in fsl fsl-business fsl-requirements fsl-design fsl-design-review fsl-delivery; do
    SKILL_SRC="$CURRENT_LINK/skills/$SKILL_NAME"
    SKILL_DST="$HOME/.claude/skills/$SKILL_NAME"
    [ -d "$RELEASE_DIR/skills/$SKILL_NAME" ] \
      || fail "$RELEASE_DIR is missing the $SKILL_NAME skill."
    if [ -L "$SKILL_DST" ] && [ "$(readlink "$SKILL_DST")" = "$SKILL_SRC" ]; then
      echo "The Claude Code skill link is current: $SKILL_DST"
    elif [ -e "$SKILL_DST" ] || [ -L "$SKILL_DST" ]; then
      SKILL_BACKUP="$SKILL_DST.pre-native-v3"
      [ ! -e "$SKILL_BACKUP" ] && [ ! -L "$SKILL_BACKUP" ] \
        || fail "$SKILL_BACKUP already exists. Move it and re-run."
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

# Every destination has been checked and prepared. Rename a same-filesystem
# temporary link over current to activate the complete payload atomically.
ACTIVATION_DIR=$(mktemp -d "$INSTALL_DIR/.activate.XXXXXX")
trap 'rm -rf "$ACTIVATION_DIR"' EXIT
ACTIVATION_LINK="$ACTIVATION_DIR/current"
ln -s "releases/$RELEASE_NAME" "$ACTIVATION_LINK"
case "$(uname -s)" in
  Darwin) mv -fh "$ACTIVATION_LINK" "$CURRENT_LINK" ;;
  Linux) mv -fT "$ACTIVATION_LINK" "$CURRENT_LINK" ;;
  *) fail "Atomic activation is unsupported on this operating system." ;;
esac
rm -rf "$ACTIVATION_DIR"
trap - EXIT
[ "$("$FSL_BIN" --version)" = "$EXPECTED_VERSION" ] \
  || fail "The active native binary does not report $EXPECTED_VERSION."
if command -v fslc >/dev/null 2>&1; then
  RESOLVED_FSL=$(command -v fslc)
  RESOLVED_VERSION=$(fslc --version 2>/dev/null || true)
  if [ "$RESOLVED_VERSION" != "$EXPECTED_VERSION" ]; then
    echo "Warning: PATH resolves fslc to $RESOLVED_FSL reporting '$RESOLVED_VERSION'." >&2
    echo "Put $LOCAL_BIN before the old Python installation to use $EXPECTED_VERSION." >&2
  fi
fi

if [ -d "$HOME/.fsl/.git" ]; then
  echo "Legacy repository detected at $HOME/.fsl. It is no longer used; remove it after preserving any local changes."
fi
echo "Installed native FSL $RELEASE_TAG without a repository clone: $INSTALL_DIR"
