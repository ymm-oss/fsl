#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
set -u
values=()
cat <<-'DATA'
	${values[@]}
	$(mapfile -t nested_values </dev/null)
	DATA
