#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Rebuild the checked-in prebuilt binary gnss/bin/tabpro-s-gnss in a Fedora container, so it links
# against Fedora's glibc (needs podman or docker). Same build as packaging/specs/tabpro-s-gnss.spec.
#   gnss/build-binary.sh [fedora release, default 45]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
rel="${1:-45}"
run="$(command -v podman || command -v docker)"
mkdir -p "$here/bin"
"$run" run --rm -v "$here/tabpro-s-gnss:/src:ro,Z" -v "$here/bin:/out:Z" "registry.fedoraproject.org/fedora:$rel" bash -c '
  set -e
  dnf install -y -q cargo rust binutils >/dev/null
  cp -r /src /tmp/b && rm -rf /tmp/b/target
  cargo build -q --release --locked --manifest-path /tmp/b/Cargo.toml
  strip -o /out/tabpro-s-gnss /tmp/b/target/release/tabpro-s-gnss'
(cd "$here/bin" && sha256sum tabpro-s-gnss > tabpro-s-gnss.sha256)
ls -l "$here/bin/tabpro-s-gnss"; cat "$here/bin/tabpro-s-gnss.sha256"
