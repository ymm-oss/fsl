#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

head_ref="${1:?usage: check-production-source-policy.sh HEAD_REF}"
if [[ "$head_ref" == main ||
      "$head_ref" =~ ^release/v[0-9]+\.[0-9]+$ ||
      "$head_ref" =~ ^hotfix/v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  exit 0
fi

echo "production accepts only main, release/vX.Y, or hotfix/vX.Y.Z" >&2
exit 1
