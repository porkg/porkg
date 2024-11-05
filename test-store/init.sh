#!/usr/bin/env bash
set -euo pipefail

script_dir=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

out_dir=$script_dir/pkg/busybox/blake3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/out

if [ ! -e "$out_dir/bin/busybox" ]; then
    mkdir -p "$out_dir/bin"
    curl -L https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox -o "$out_dir/bin/busybox"
    chmod +x "$out_dir/bin/busybox"
fi

mkdir -p "$out_dir/bin"

for exe in ash sh cat chgrp chmod chown cp cut true false echo; do
    ln -sf "./busybox" "$out_dir/bin/$exe"
done
