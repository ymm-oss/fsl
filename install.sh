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

if ! command -v python3 >/dev/null 2>&1; then
  fail "Python 3.9 or later is required. Install Python 3 from https://www.python.org/ and re-run."
fi

PYTHON_BIN=$(command -v python3)
if ! "$PYTHON_BIN" -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 9) else 1)' >/dev/null 2>&1; then
  PY_VERSION=$("$PYTHON_BIN" -c 'import sys; print(".".join(map(str, sys.version_info[:3])))' 2>/dev/null || echo "unknown")
  fail "Python 3.9 or later is required (current: ${PY_VERSION}). Update from https://www.python.org/ and re-run."
fi

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

VENV_DIR="$REPO_DIR/.venv"
VENV_PYTHON="$VENV_DIR/bin/python"
FSL_BIN="$VENV_DIR/bin/fslc"

echo "Preparing the Python virtual environment: $VENV_DIR"
PYTHONPATH= "$PYTHON_BIN" -m venv "$VENV_DIR" || fail "Failed to create the Python virtual environment. On Linux where python3-venv is required, install it via your OS package manager and re-run."

[ -x "$VENV_PYTHON" ] || fail "$VENV_PYTHON not found. Remove $VENV_DIR and re-run."

echo "Upgrading pip."
PYTHONPATH= "$VENV_PYTHON" -m pip install --upgrade pip || fail "Failed to upgrade pip. Check your network connection or Python environment and re-run."

echo "Installing fslc."
PIP_INSTALL_LOG=$(mktemp)
if PYTHONPATH= "$VENV_PYTHON" -m pip install "$REPO_DIR" >"$PIP_INSTALL_LOG" 2>&1; then
  rm -f "$PIP_INSTALL_LOG"
else
  rm -f "$PIP_INSTALL_LOG"
  echo "pip install . failed, so falling back to a local placement using the existing dependencies."
  if ! PYTHONPATH= "$VENV_PYTHON" -c 'import lark, z3' >/dev/null 2>&1; then
    fail "The dependencies lark and z3-solver are required. Check your network connection and re-run."
  fi
  SITE_PACKAGES=$(PYTHONPATH= "$VENV_PYTHON" -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')
  case "$SITE_PACKAGES" in
    "$VENV_DIR"/*)
      ;;
    *)
      fail "Cannot safely determine the site-packages location. Check $VENV_DIR and re-run."
      ;;
  esac
  rm -rf "$SITE_PACKAGES/fslc" "$SITE_PACKAGES/fslc-1.1.0.dist-info"
  cp -R "$REPO_DIR/src/fslc" "$SITE_PACKAGES/fslc"
  mkdir -p "$SITE_PACKAGES/fslc-1.1.0.dist-info"
  {
    echo "Metadata-Version: 2.1"
    echo "Name: fslc"
    echo "Version: 1.1.0"
  } > "$SITE_PACKAGES/fslc-1.1.0.dist-info/METADATA"
  {
    echo "fslc"
  } > "$SITE_PACKAGES/fslc-1.1.0.dist-info/top_level.txt"
  cat > "$FSL_BIN" <<EOF
#!$VENV_PYTHON
from fslc.cli import main
raise SystemExit(main())
EOF
  chmod +x "$FSL_BIN"
fi

[ -x "$FSL_BIN" ] || fail "$FSL_BIN not found. Check the result of pip install and re-run."

LOCAL_BIN="$HOME/.local/bin"
LINK_PATH="$LOCAL_BIN/fslc"
mkdir -p "$LOCAL_BIN"

if [ -L "$LINK_PATH" ]; then
  LINK_TARGET=$(readlink "$LINK_PATH" || true)
  if [ "$LINK_TARGET" = "$FSL_BIN" ]; then
    echo "The fslc command link is already set up: $LINK_PATH"
  else
    echo "Warning: $LINK_PATH points to a different location (${LINK_TARGET}). Not overwriting."
  fi
elif [ -e "$LINK_PATH" ]; then
  echo "Warning: $LINK_PATH already exists. Not overwriting. If needed, remove it manually and re-run."
else
  ln -s "$FSL_BIN" "$LINK_PATH"
  echo "Created the fslc command link: $LINK_PATH"
fi

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
  SKILL_SRC="$REPO_DIR/skills/fsl"
  SKILL_DST="$HOME/.claude/skills/fsl"
  [ -d "$SKILL_SRC" ] || fail "$SKILL_SRC not found. Check that the repository was fetched correctly and re-run."
  if [ -e "$SKILL_DST" ] || [ -L "$SKILL_DST" ]; then
    echo "The Claude Code skill already exists: $SKILL_DST — to update it, run rm -rf \"$SKILL_DST\" and re-run."
  else
    mkdir -p "$(dirname "$SKILL_DST")"
    cp -R "$SKILL_SRC" "$SKILL_DST"
    echo "Copied the Claude Code skill: $SKILL_DST"
  fi
else
  echo "Skipped copying the Claude Code skill."
fi

echo "Running a smoke test."
CHECK_OUTPUT=$(PYTHONPATH= "$FSL_BIN" check "$REPO_DIR/specs/cart_v1.fsl") || fail "Smoke test failed. Check the result of $FSL_BIN check $REPO_DIR/specs/cart_v1.fsl."
case "$CHECK_OUTPUT" in
  *'"result": "ok"'*|*'"result":"ok"'*)
    echo "Smoke test ok: fslc check specs/cart_v1.fsl"
    ;;
  *)
    fail "Smoke test was not ok. Check the output of $FSL_BIN check $REPO_DIR/specs/cart_v1.fsl."
    ;;
esac

echo "Done. Open Claude Code and try saying something like \"verify this business flow with FSL.\" Examples: $REPO_DIR/examples/pm/ (for PMs), $REPO_DIR/examples/consulting/ (for consultants)"
