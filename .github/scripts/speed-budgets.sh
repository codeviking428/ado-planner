#!/usr/bin/env bash
# Run the #53 speed budgets (crates/app/tests/speed.rs and launch.rs) in
# release. See docs/speed-budgets.md.
#
#   dbus-run-session -- .github/scripts/speed-budgets.sh [runs]
#
# The launch test relaunches the real app with a saved Session, so it needs
# a Secret Service keyring (an unlocked gnome-keyring on this D-Bus session),
# an X display (Xvfb) and a Vulkan adapter (Mesa's llvmpipe). `runs` repeats
# the whole suite, for calibrating budgets; the default is one.
set -euo pipefail

runs="${1:-1}"
root="$(git rev-parse --show-toplevel)"
cd "$root"

if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  echo "speed-budgets: run under dbus-run-session" >&2
  exit 1
fi

# A fresh keyring directory, so a login keyring with another password cannot
# leave the collection locked behind a prompt nobody answers.
keyrings="$(mktemp -d)"
export XDG_DATA_HOME="$keyrings"
printf 'ci' | gnome-keyring-daemon --unlock --components=secrets >/dev/null
unset XDG_DATA_HOME

export ADO_PLANNER_SPEED_TESTS=1
status=0
for run in $(seq 1 "$runs"); do
  echo "::group::speed budgets, run $run of $runs"
  xvfb-run -a -s "-screen 0 1280x800x24" \
    cargo test --release --locked -p ado-planner --test speed --test launch \
    -- --nocapture --test-threads 1 || status=1
  echo "::endgroup::"
done
exit "$status"
