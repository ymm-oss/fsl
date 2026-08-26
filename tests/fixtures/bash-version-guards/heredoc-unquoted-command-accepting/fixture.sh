#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
cat <<DATA
readarray -t values </dev/null
local -A labels=()
DATA
