# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Suite-wide test fixtures.

The verdict cache (issue #169, ``fslc.verify_cache``) defaults to on and
reads/writes ``~/.cache/fslc`` (or ``$FSLC_CACHE_DIR``). The test suite must
never observe a developer's warm cache, and must never write into it --
``tests/test_verify_cache.py`` opts back in per-test via
``monkeypatch.setenv("FSLC_CACHE_DIR", tmp_path)``.
"""
import pytest


@pytest.fixture(autouse=True)
def _disable_verify_cache(monkeypatch):
    monkeypatch.setenv("FSLC_CACHE", "off")
    monkeypatch.delenv("FSLC_CACHE_DIR", raising=False)
    monkeypatch.delenv("FSLC_CACHE_VERIFY", raising=False)
