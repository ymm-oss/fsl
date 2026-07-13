# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Export the complete public ``fslc`` argparse surface as canonical JSON."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from fslc.cli import _build_arg_parser
from fslc.cli_help import canonical_help


ROOT = Path(__file__).resolve().parents[1]


def _subparsers(parser: argparse.ArgumentParser) -> argparse._SubParsersAction | None:
    return next(
        (
            action
            for action in parser._actions  # noqa: SLF001 - argparse is the contract source
            if isinstance(action, argparse._SubParsersAction)  # noqa: SLF001
        ),
        None,
    )


def _action_contract(action: argparse.Action) -> dict[str, Any]:
    contract: dict[str, Any] = {
        "dest": action.dest,
        "required": bool(getattr(action, "required", False)),
    }
    if action.option_strings:
        contract["flags"] = list(action.option_strings)
    else:
        contract["positional"] = True
    if action.nargs is not None:
        contract["nargs"] = action.nargs
    if action.choices is not None:
        contract["choices"] = list(action.choices)
    if action.default is not None and action.default is not argparse.SUPPRESS:
        contract["default"] = action.default
    if isinstance(action, (argparse._AppendAction, argparse._AppendConstAction)):  # noqa: SLF001
        contract["repeatable"] = True
    if isinstance(action, argparse._StoreTrueAction):  # noqa: SLF001
        contract["action"] = "store_true"
    elif isinstance(action, argparse._StoreFalseAction):  # noqa: SLF001
        contract["action"] = "store_false"
    elif isinstance(action, argparse._VersionAction):  # noqa: SLF001
        contract["action"] = "version"
    else:
        contract["action"] = "store"
    return contract


def _parser_contract(parser: argparse.ArgumentParser, path: tuple[str, ...]) -> dict[str, Any]:
    nested = _subparsers(parser)
    actions = [
        action
        for action in parser._actions  # noqa: SLF001 - argparse is the contract source
        if action is not nested
    ]
    return {
        "path": list(path),
        "prog": parser.prog,
        "help": canonical_help(parser),
        "actions": [_action_contract(action) for action in actions],
        "commands": [
            _parser_contract(child, (*path, name))
            for name, child in sorted((nested.choices if nested else {}).items())
        ],
    }


def export_contract() -> dict[str, Any]:
    """Return the deterministic public CLI contract."""
    return {
        "schema": "fsl-cli-contract.v1",
        "root": _parser_contract(_build_arg_parser(), ()),
    }


def main() -> None:
    import argparse as cli_argparse

    parser = cli_argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-o", "--output", type=Path)
    args = parser.parse_args()
    rendered = json.dumps(export_contract(), ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
