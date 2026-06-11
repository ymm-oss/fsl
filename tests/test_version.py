"""fslc にバージョン表示があり、ライブラリ版・パッケージ版と一致すること。"""
import subprocess
import sys
from pathlib import Path

import fslc

ROOT = Path(__file__).resolve().parent.parent
PY = ROOT / ".venv" / "bin" / "python"


def _run(*args):
    return subprocess.run([str(PY), "-m", "fslc", *args],
                          capture_output=True, text=True, cwd=ROOT)


def test_version_subcommand():
    r = _run("version")
    assert r.returncode == 0
    assert r.stdout.strip() == f"fslc {fslc.__version__}"


def test_version_flag_long_and_short():
    for flag in ("--version", "-V"):
        r = _run(flag)
        assert r.returncode == 0, flag
        assert r.stdout.strip() == f"fslc {fslc.__version__}", flag


def test_library_version_is_nonempty_string():
    assert isinstance(fslc.__version__, str) and fslc.__version__
