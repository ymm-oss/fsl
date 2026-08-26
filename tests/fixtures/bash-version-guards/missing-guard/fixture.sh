#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
set -u
values=()
declare -A labels=()
mapfile -t values </dev/null
printf '%s\n' "${values[@]}"
