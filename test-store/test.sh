#!/usr/bin/env bash
set -euo pipefail

script_dir=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

curl
    --unix-socket "$script_dir/../target/runtime/porkg.socket" \
    -v http:/a/api/v1/build \
    -H 'content-type: application/json' \
    -d '{
    "name": "test",
    "hash": "blake3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }'
