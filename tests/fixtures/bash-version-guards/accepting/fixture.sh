#!/usr/bin/env bash
(( BASH_VERSINFO[0] >= 4 )) || { echo "fixture requires Bash 4 or newer" >&2; exit 1; }
# SPDX-License-Identifier: Apache-2.0
set -u
values=()
declare -A labels=()
mapfile -t values </dev/null
printf '%s\n' "${values[@]}"
