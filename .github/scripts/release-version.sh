#!/usr/bin/env bash
# Read, check, and bump the release version. See docs/RELEASING.md.
#
#   release-version.sh current            print the workspace version
#   release-version.sh check              every crate and package.json share it
#   release-version.sh bump X.Y.Z         set it in Cargo.toml, Cargo.lock, package.json
#   release-version.sh changelog vX.Y.Z   prepend the git-cliff section to CHANGELOG.md
#   release-version.sh notes vX.Y.Z       print that version's CHANGELOG.md section
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

semver_re='^[0-9]+\.[0-9]+\.[0-9]+$'

die() {
  echo "release-version: $*" >&2
  exit 1
}

package_versions() {
  cargo metadata --no-deps --format-version 1 --locked |
    jq -r '.packages[] | "\(.name) \(.version)"'
}

current() {
  package_versions | awk '$1 == "ado-planner" { print $2 }'
}

check() {
  local want versions npm
  want="$(current)"
  [[ -n "$want" ]] || die "ado-planner is not a workspace member"
  versions="$(package_versions)"
  if awk -v want="$want" '$2 != want { bad = 1; print "  " $0 } END { exit !bad }' <<<"$versions" >&2; then
    die "crates above do not use version.workspace = true (want $want)"
  fi
  npm="$(jq -r .version package.json)"
  [[ "$npm" == "$want" ]] || die "package.json is $npm but Cargo.toml is $want"
  echo "$want"
}

bump() {
  local next="${1:?usage: bump X.Y.Z}"
  next="${next#v}"
  [[ "$next" =~ $semver_re ]] || die "not a plain SemVer: $next"
  sed -i.bak -E '/^\[workspace\.package\]/,/^\[/ s/^version = "[^"]*"/version = "'"$next"'"/' Cargo.toml
  rm -f Cargo.toml.bak
  jq --arg v "$next" '.version = $v' package.json >package.json.tmp
  mv package.json.tmp package.json
  cargo update --workspace --quiet
  [[ "$(check)" == "$next" ]] || die "bump to $next did not apply"
}

changelog() {
  local tag="${1:?usage: changelog vX.Y.Z}" section
  section="$(git cliff --unreleased --tag "$tag" --strip header 2>/dev/null)"
  [[ -n "$section" ]] || die "git-cliff produced no entries for $tag"
  [[ -f CHANGELOG.md ]] || die "CHANGELOG.md is missing"
  SECTION="$section" awk '
    !done && /^## / { print ENVIRON["SECTION"]; print ""; done = 1 }
    { print }
    END { if (!done) { print ""; print ENVIRON["SECTION"] } }
  ' CHANGELOG.md >CHANGELOG.md.tmp
  mv CHANGELOG.md.tmp CHANGELOG.md
}

notes() {
  local version="${1:?usage: notes vX.Y.Z}"
  version="${version#v}"
  awk -v v="$version" '
    /^## / { if (found) exit; found = index($0, "## [" v "]") == 1; next }
    found && (started || NF) { started = 1; print }
  ' CHANGELOG.md
}

cmd="${1:-}"
shift || true
case "$cmd" in
current) current ;;
check) check ;;
bump) bump "$@" ;;
changelog) changelog "$@" ;;
notes) notes "$@" ;;
*) die "unknown command '${cmd}' (current, check, bump, changelog, notes)" ;;
esac
