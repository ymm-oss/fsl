# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""PyInstaller entry point for the standalone ``fslc`` binary.

PyInstaller freezes this module (not ``-m fslc``) so the resulting one-file
executable has a stable, importable entry. The actual CLI lives in
``fslc.cli.main``.
"""
from fslc.cli import main

if __name__ == "__main__":
    main()
