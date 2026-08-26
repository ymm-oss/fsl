#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
cat <<'QUOTED_DATA'
mapfile -t values </dev/null
QUOTED_DATA
cat <<PLAIN_DATA
local -A labels=()
PLAIN_DATA
cat <<-TAB_DATA
	readarray -t values </dev/null
	TAB_DATA
