# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Language-server support for FSL."""

from .index import (
    DocumentIndex,
    ImportBinding,
    Location,
    Position,
    Range,
    Reference,
    Symbol,
    build_index,
    default_load_index,
    definition_at,
)

__all__ = [
    "DocumentIndex",
    "ImportBinding",
    "Location",
    "Position",
    "Range",
    "Reference",
    "Symbol",
    "build_index",
    "default_load_index",
    "definition_at",
]
