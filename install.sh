#!/usr/bin/env bash
set -euo pipefail

INSTALL_SKILL=1
CLONE_URL="git@github.com:yumemi/fsl.git"
INSTALL_DIR="${FSL_INSTALL_DIR:-$HOME/.fsl}"

fail() {
  echo "エラー: $*" >&2
  exit 1
}

usage() {
  echo "使い方: bash install.sh [--no-skill]"
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
      fail "不明なオプションです: $1。使えるオプションは --no-skill だけです。"
      ;;
  esac
  shift
done

if ! command -v python3 >/dev/null 2>&1; then
  fail "Python 3.9 以上が必要です。https://www.python.org/ から Python 3 をインストールしてから再実行してください。"
fi

PYTHON_BIN=$(command -v python3)
if ! "$PYTHON_BIN" -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 9) else 1)' >/dev/null 2>&1; then
  PY_VERSION=$("$PYTHON_BIN" -c 'import sys; print(".".join(map(str, sys.version_info[:3])))' 2>/dev/null || echo "unknown")
  fail "Python 3.9 以上が必要です（現在: ${PY_VERSION}）。https://www.python.org/ から更新してから再実行してください。"
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
    fail "git が必要です。https://git-scm.com/ からインストールするか、GitHub からリポジトリを取得して ./install.sh を実行してください。"
  fi

  if [ -e "$INSTALL_DIR" ]; then
    [ -d "$INSTALL_DIR/.git" ] || fail "$INSTALL_DIR は既にありますが Git リポジトリではありません。削除または移動してから再実行してください。"
    echo "FSL リポジトリを更新しています: $INSTALL_DIR"
    git -C "$INSTALL_DIR" pull --ff-only || fail "$INSTALL_DIR の更新に失敗しました。ローカル変更を確認するか、削除してから再実行してください。"
  else
    echo "FSL リポジトリを取得しています: $INSTALL_DIR"
    # 社内(private)リポジトリのため認証が必要。gh CLI があればそれが一番確実
    if command -v gh >/dev/null 2>&1; then
      gh repo clone yumemi/fsl "$INSTALL_DIR" || fail "リポジトリの取得に失敗しました。'gh auth login' で GitHub にログインしているか、yumemi/fsl へのアクセス権があるかを確認してください。"
    else
      git clone "$CLONE_URL" "$INSTALL_DIR" || fail "リポジトリの取得に失敗しました。yumemi/fsl は社内リポジトリのため認証が必要です。GitHub CLI('brew install gh' → 'gh auth login')を入れて再実行するのが簡単です。SSH キー設定済みの方はそのままで動きます。"
    fi
  fi
  REPO_DIR=$(cd "$INSTALL_DIR" && pwd -P)
else
  REPO_DIR=$(cd "$REPO_DIR" && pwd -P)
  if [ -d "$REPO_DIR/.git" ]; then
    # 開発者の作業ツリー(git チェックアウト): その場で使う
    echo "このリポジトリを使います: $REPO_DIR"
  elif [ "$REPO_DIR" = "$INSTALL_DIR" ]; then
    # 既に $INSTALL_DIR に配置済み(再実行)
    echo "インストール済みのフォルダを使います: $REPO_DIR"
  else
    case "$REPO_DIR/" in
      "$INSTALL_DIR"/*)
        # 万一 $INSTALL_DIR の中から実行された場合はその場で使う
        echo "このフォルダを使います: $REPO_DIR"
        ;;
      *)
        # ZIP などで展開したソース: 安定した場所($INSTALL_DIR)へ配置してから使う
        echo "ダウンロードしたフォルダから $INSTALL_DIR へ配置しています。"
        SRC_DIR="$REPO_DIR"
        mkdir -p "$INSTALL_DIR"
        # .venv は残す(再実行時に環境を保つ)、ソースは入れ替える
        find "$INSTALL_DIR" -mindepth 1 -maxdepth 1 ! -name .venv -exec rm -rf {} + 2>/dev/null || true
        (
          cd "$SRC_DIR" && for item in * .[!.]*; do
            case "$item" in .venv|.git) continue ;; esac
            [ -e "$item" ] || continue
            cp -R "$item" "$INSTALL_DIR/"
          done
        ) || fail "$INSTALL_DIR への配置に失敗しました。$INSTALL_DIR を削除してから再実行してください。"
        REPO_DIR=$(cd "$INSTALL_DIR" && pwd -P)
        is_fsl_repo "$REPO_DIR" || fail "$INSTALL_DIR への配置に失敗しました。$INSTALL_DIR を削除してから再実行してください。"
        echo "配置しました: ${REPO_DIR}（ダウンロードしたフォルダは削除して構いません）"
        ;;
    esac
  fi
fi

VENV_DIR="$REPO_DIR/.venv"
VENV_PYTHON="$VENV_DIR/bin/python"
FSL_BIN="$VENV_DIR/bin/fslc"

echo "Python 仮想環境を準備しています: $VENV_DIR"
PYTHONPATH= "$PYTHON_BIN" -m venv "$VENV_DIR" || fail "Python 仮想環境の作成に失敗しました。python3-venv が必要な Linux では OS のパッケージで追加してから再実行してください。"

[ -x "$VENV_PYTHON" ] || fail "$VENV_PYTHON が見つかりません。$VENV_DIR を削除してから再実行してください。"

echo "pip を更新しています。"
PYTHONPATH= "$VENV_PYTHON" -m pip install --upgrade pip || fail "pip の更新に失敗しました。ネットワーク接続または Python 環境を確認してから再実行してください。"

echo "fslc をインストールしています。"
PIP_INSTALL_LOG=$(mktemp)
if PYTHONPATH= "$VENV_PYTHON" -m pip install "$REPO_DIR" >"$PIP_INSTALL_LOG" 2>&1; then
  rm -f "$PIP_INSTALL_LOG"
else
  rm -f "$PIP_INSTALL_LOG"
  echo "pip install . に失敗したため、既存の依存パッケージを使ってローカル配置します。"
  if ! PYTHONPATH= "$VENV_PYTHON" -c 'import lark, z3' >/dev/null 2>&1; then
    fail "依存パッケージ lark と z3-solver が必要です。ネットワーク接続を確認してから再実行してください。"
  fi
  SITE_PACKAGES=$(PYTHONPATH= "$VENV_PYTHON" -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')
  case "$SITE_PACKAGES" in
    "$VENV_DIR"/*)
      ;;
    *)
      fail "site-packages の場所を安全に確認できません。$VENV_DIR を確認してから再実行してください。"
      ;;
  esac
  rm -rf "$SITE_PACKAGES/fslc" "$SITE_PACKAGES/fslc-1.0.2.dist-info"
  cp -R "$REPO_DIR/src/fslc" "$SITE_PACKAGES/fslc"
  mkdir -p "$SITE_PACKAGES/fslc-1.0.2.dist-info"
  {
    echo "Metadata-Version: 2.1"
    echo "Name: fslc"
    echo "Version: 1.0.2"
  } > "$SITE_PACKAGES/fslc-1.0.2.dist-info/METADATA"
  {
    echo "fslc"
  } > "$SITE_PACKAGES/fslc-1.0.2.dist-info/top_level.txt"
  cat > "$FSL_BIN" <<EOF
#!$VENV_PYTHON
from fslc.cli import main
raise SystemExit(main())
EOF
  chmod +x "$FSL_BIN"
fi

[ -x "$FSL_BIN" ] || fail "$FSL_BIN が見つかりません。pip install の結果を確認してから再実行してください。"

LOCAL_BIN="$HOME/.local/bin"
LINK_PATH="$LOCAL_BIN/fslc"
mkdir -p "$LOCAL_BIN"

if [ -L "$LINK_PATH" ]; then
  LINK_TARGET=$(readlink "$LINK_PATH" || true)
  if [ "$LINK_TARGET" = "$FSL_BIN" ]; then
    echo "fslc コマンドのリンクは既に設定済みです: $LINK_PATH"
  else
    echo "警告: $LINK_PATH は別の場所を指しています（${LINK_TARGET}）。上書きしません。"
  fi
elif [ -e "$LINK_PATH" ]; then
  echo "警告: $LINK_PATH は既に存在します。上書きしません。必要なら手動で削除して再実行してください。"
else
  ln -s "$FSL_BIN" "$LINK_PATH"
  echo "fslc コマンドのリンクを作成しました: $LINK_PATH"
fi

case ":$PATH:" in
  *":$LOCAL_BIN:"*)
    ;;
  *)
    SHELL_NAME=$(basename "${SHELL:-}")
    if [ "$SHELL_NAME" = "zsh" ]; then
      echo "PATH に $LOCAL_BIN がありません。必要なら次の1行を ~/.zshrc に追加してください: export PATH=\"\$HOME/.local/bin:\$PATH\""
    elif [ "$SHELL_NAME" = "bash" ]; then
      echo "PATH に $LOCAL_BIN がありません。必要なら次の1行を ~/.bashrc に追加してください: export PATH=\"\$HOME/.local/bin:\$PATH\""
    else
      echo "PATH に $LOCAL_BIN がありません。必要ならシェルの設定ファイルに次の1行を追加してください: export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
    ;;
esac

if [ "$INSTALL_SKILL" -eq 1 ]; then
  SKILL_SRC="$REPO_DIR/skills/fsl"
  SKILL_DST="$HOME/.claude/skills/fsl"
  [ -d "$SKILL_SRC" ] || fail "$SKILL_SRC が見つかりません。リポジトリの取得状態を確認してから再実行してください。"
  if [ -e "$SKILL_DST" ] || [ -L "$SKILL_DST" ]; then
    echo "Claude Code 用スキルは既にあります: $SKILL_DST — 更新する場合は rm -rf \"$SKILL_DST\" して再実行してください。"
  else
    mkdir -p "$(dirname "$SKILL_DST")"
    cp -R "$SKILL_SRC" "$SKILL_DST"
    echo "Claude Code 用スキルをコピーしました: $SKILL_DST"
  fi
else
  echo "Claude Code 用スキルのコピーをスキップしました。"
fi

echo "動作確認を実行しています。"
CHECK_OUTPUT=$(PYTHONPATH= "$FSL_BIN" check "$REPO_DIR/specs/cart_v1.fsl") || fail "動作確認に失敗しました。$FSL_BIN check $REPO_DIR/specs/cart_v1.fsl の結果を確認してください。"
case "$CHECK_OUTPUT" in
  *'"result": "ok"'*|*'"result":"ok"'*)
    echo "動作確認 ok: fslc check specs/cart_v1.fsl"
    ;;
  *)
    fail "動作確認が ok ではありません。$FSL_BIN check $REPO_DIR/specs/cart_v1.fsl の出力を確認してください。"
    ;;
esac

echo "完了しました。Claude Code を開いて『業務フローを FSL で検証して』のように話しかけてください。実例: $REPO_DIR/examples/pm/（PM向け）、$REPO_DIR/examples/consulting/（コンサル向け）"
