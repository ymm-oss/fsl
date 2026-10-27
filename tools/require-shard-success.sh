#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
if [[ "${1-}" == success ]]; then
  exit 0
fi
echo "shards must succeed; result: '${1-}'" >&2
exit 1
