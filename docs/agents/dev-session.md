# Dev session

Two loops. Pick by what you need to observe.

## Tests — scripted iteration

`crates/app/tests/` drive `PlannerState` and a rendered `PlannerView` against `ado-mock` with GPUI's `TestAppContext` and `VisualTestContext`: sign in, pick a Scope, drag a bar, open the form, read painted bounds. Use them for red-green work, regressions, and anything that must be deterministic.

```bash
xvfb-run -a cargo nextest run -p ado-planner --test view   # drop xvfb-run where a display exists
cargo nextest run -p ado-mock                              # mock routes and fixtures
```

The mock serves the JSON in `crates/ado-mock/fixtures`. Change a fixture there, not in a test.

Done when the test passes, or fails with an assertion.

## The real window

GPUI draws natively, so there is no DevTools port to attach to. To see what the user sees, run your own copy against the mock:

```bash
cargo run -p ado-mock   # prints ADO_PLANNER_ADO_BASE_URL=… and ADO_PLANNER_VSSPS_URL=…; leave it running
env ADO_PLANNER_ADO_BASE_URL=http://127.0.0.1:<port> ADO_PLANNER_VSSPS_URL=http://127.0.0.1:<port> \
  ADO_PLANNER_DATA_DIR="$(mktemp -d)" RUST_LOG=debug cargo run -p ado-planner
```

Sign in with organization `https://dev.azure.com/contoso` and any token. `ADO_PLANNER_DATA_DIR` keeps prefs, cache, and the Session out of the user's data folder. Only one ADO Planner runs at a time: if the user's copy is open, yours prints `ADO Planner is already running` and exits.

Without a desktop, start `Xvfb :99 -screen 0 1600x1000x24 &` and set `DISPLAY=:99`. Without a GPU, the window needs Mesa's software Vulkan driver (`apt install mesa-vulkan-drivers`), or opening it fails with `Failed to create surface`. Drive the window with `xdotool` and capture it with `import -window root shot.png` (ImageMagick). `ADO_PLANNER_DEBUG_HIT=1` logs which zone of a Gantt bar each press hits.

Done when you have a screenshot or a log line that shows the behavior.

## The user's window

You can't attach to the user's running app. Ask for a screenshot or their `RUST_LOG=debug` output, then reproduce it with the loop above.
