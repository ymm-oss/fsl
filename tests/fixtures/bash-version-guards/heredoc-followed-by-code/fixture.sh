#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
cat <<'DATA'
mapfile is data here
DATA
mapfile -t values </dev/null
