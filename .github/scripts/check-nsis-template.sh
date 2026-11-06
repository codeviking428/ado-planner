#!/usr/bin/env bash
# Check that ADO Planner's NSIS template is cargo-packager's upstream
# installer.nsi plus marked blocks. See crates/app/packaging/windows/installer.nsi.
#
#   check-nsis-template.sh [cargo-packager version]
set -euo pipefail

version="${1:-0.11.8}"
root="$(git rev-parse --show-toplevel)"
ours="$root/crates/app/packaging/windows/installer.nsi"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl -sSfL "https://crates.io/api/v1/crates/cargo-packager/$version/download" -o "$work/crate.tar.gz"
tar -xzf "$work/crate.tar.gz" -C "$work" "cargo-packager-$version/src/package/nsis/installer.nsi"
upstream="$work/cargo-packager-$version/src/package/nsis/installer.nsi"

# Drop every `; >>> ado-planner` ... `; <<< ado-planner` block, keeping its
# `; upstream: ` lines with that prefix removed.
awk '
  /^; >>> ado-planner/ { if (inside) { print FILENAME ": nested block at line " NR > "/dev/stderr"; exit 2 } inside = 1; next }
  /^; <<< ado-planner/ { if (!inside) { print FILENAME ": stray end at line " NR > "/dev/stderr"; exit 2 } inside = 0; next }
  inside && /^; upstream: / { print substr($0, 13); next }
  inside { next }
  { print }
  END { if (inside) { print FILENAME ": unterminated block" > "/dev/stderr"; exit 2 } }
' "$ours" >"$work/restored.nsi"

if ! diff -u "$upstream" "$work/restored.nsi"; then
  echo "check-nsis-template: $ours differs from cargo-packager $version outside its ado-planner blocks" >&2
  exit 1
fi
echo "NSIS template matches cargo-packager $version outside $(grep -c '^; >>> ado-planner' "$ours") ado-planner blocks"
