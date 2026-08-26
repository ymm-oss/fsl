#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# A guard after another line is too late.
(( BASH_VERSINFO[0] >= 4 )) || { echo "fixture requires Bash 4 or newer" >&2; exit 1; }
mapfile -t values </dev/null
