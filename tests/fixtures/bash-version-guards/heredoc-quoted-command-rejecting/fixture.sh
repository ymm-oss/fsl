#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
cat <<'DATA'
mapfile is literal data
DATA
mapfile -t values </dev/null
