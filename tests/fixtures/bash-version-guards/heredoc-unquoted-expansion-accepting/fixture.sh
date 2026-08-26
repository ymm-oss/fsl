#!/usr/bin/env bash
(( BASH_VERSINFO[0] >= 4 )) || { echo "Bash 4+ required" >&2; exit 1; }
# SPDX-License-Identifier: Apache-2.0
set -u
values=()
cat <<-DATA
	${values[@]}
	$(mapfile -t nested_values </dev/null)
	DATA
