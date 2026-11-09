# Speed budgets

The `Speed budgets` CI job fails a pull request when the Rust app regresses past one of these budgets ([#53](https://github.com/codeviking428/ado-planner/issues/53)). Frame-by-frame history is in [performance.md](performance.md).

| Budget                            |       #53 target | CI fails above | Hosted runners measured | Target met           |
| --------------------------------- | ---------------: | -------------: | ----------------------: | -------------------- |
| First frame after launch          |           300 ms |         300 ms |              128–152 ms | yes                  |
| Gantt from warm cache, 5,000 rows |           500 ms |         650 ms |              432–505 ms | on one runner of two |
| Scroll frame, 5,000 rows          | 16.7 ms (60 fps) |          46 ms |            35.8–36.5 ms | no                   |
| Drag frame, 5,000 rows            | 16.7 ms (60 fps) |          29 ms |            22.9–23.2 ms | no                   |
| Idle memory                       |           150 MB | 150 MB private |      128–137 MB private | yes                  |
| Idle memory, all resident         |                — |         350 MB |              269–279 MB | —                    |

The Electron app's numbers are not measured here. Jonathan compares them by hand in his parity pass.

## What each budget measures

**Launch budgets** (`crates/app/tests/launch.rs`) run the real release binary:

1. `ado-mock` serves 5,000 ADO-shaped work items: Epic → 4 Features → 5 User Stories → 4 Tasks, the same dataset as the frame budgets.
2. The test signs in once with a PAT, as a first launch would. That leaves the encrypted Session in the keyring-backed store, the chosen Team in prefs, and a warm query cache on disk.
3. It launches the app once to warm up, then five more times. With `ADO_PLANNER_STARTUP_PROBE=1` the app prints a line when its first frame has been presented, and another when the Gantt has painted the loaded rows. The test times each line from spawn.
4. Two seconds after the Gantt paints, it reads `/proc/<pid>/status`. Each budget takes the median of the five launches.

- **First frame** is spawn to the first presented frame. It includes window and GPU setup and the Session restore from the keyring.
- **Gantt from warm cache** is spawn to the Gantt painting 5,000 rows. The query cache serves orgs, projects and Teams, but the app always refetches the Hierarchy (`PlannerState::load_hierarchy`), so this includes 50 loopback requests to the mock.
- **Idle memory** is the anonymous resident memory (`RssAnon`): heap, stacks and other pages no other process shares. That is the closest Linux reading of Task Manager's default Memory column, the private working set. All resident memory (`VmRSS`) is about 140 MB higher. Roughly 85 MB of that is Mesa's software renderer (`libLLVM`, lavapipe, its shared buffers), which a machine with a GPU does not load. So the 150 MB target applies to private memory, and all resident memory has only a regression guard.

**Frame budgets** (`crates/app/tests/speed.rs`) paint a real `PlannerView` over the same 5,000 rows at 1280×800 in the GPUI test platform. They take medians of 40 wheel-scroll frames and 40 bar-drag frames, three times, and each budget takes the median of the three. The test platform does layout and paint bookkeeping but no GPU work, so these are CPU frame-build times.

The job runs on GitHub's `ubuntu-latest` (Ubuntu 26.04) under Xvfb, with Mesa's llvmpipe as the Vulkan adapter and an unlocked `gnome-keyring` on a `dbus-run-session` bus. Hosted runners for public repositories have 4 vCPUs and 16 GB of RAM ([GitHub docs](https://docs.github.com/en/actions/using-github-hosted-runners/using-github-hosted-runners/about-github-hosted-runners#standard-github-hosted-runners-for-public-repositories)).

## How the budgets were chosen

The suite ran three times on each of two hosted runners, on 2026-11-09, for commits `8dd110b` and `678134e` of [#126](https://github.com/codeviking428/ado-planner/pull/126). Each row is the median of one run:

| Runner | Run | First frame |    Gantt |  Private | All resident |  Scroll |    Drag |
| ------ | --: | ----------: | -------: | -------: | -----------: | ------: | ------: |
| A      |   1 |    151.8 ms | 503.8 ms | 129.7 MB |     271.4 MB |       — |       — |
| A      |   2 |    151.1 ms | 503.1 ms | 137.3 MB |     278.7 MB |       — |       — |
| A      |   3 |    151.3 ms | 504.5 ms | 127.9 MB |     269.2 MB |       — |       — |
| B      |   1 |    128.3 ms | 432.3 ms | 128.1 MB |     269.5 MB | 35.8 ms | 23.0 ms |
| B      |   2 |    128.5 ms | 433.8 ms | 130.7 MB |     271.8 MB | 35.9 ms | 22.9 ms |
| B      |   3 |    128.6 ms | 433.1 ms | 132.8 MB |     273.5 MB | 36.5 ms | 23.2 ms |

On runner A, a failed launch budget stopped `cargo test` before the frame budgets ran; the job now passes `--no-fail-fast`.

Runs on the same runner agreed within 3%. The two runners differed by about 17%. Each budget is therefore the slowest median above plus 25%, rounded up. Where #53's target is stricter than that, the target is the budget. That gives:

- **First frame:** 152 ms + 25% is 190 ms, so the 300 ms target applies.
- **Gantt:** 505 ms + 25% is 631 ms, so the budget is 650 ms.
- **Scroll:** 36.5 ms + 25% is 45.6 ms, so the budget is 46 ms.
- **Drag:** 23.2 ms + 25% is 29.0 ms, so the budget is 29 ms.
- **All resident memory:** 279 MB + 25% is 348 MB, so the budget is 350 MB.
- **Private memory** does not scale with runner speed. The 150 MB target leaves 9% over the slowest median and 5% over the worst single launch (142.6 MB), so it stays.

A budget fails when the median of a run is over it, not a single launch or frame.

## Where #53's targets are not met

- **Gantt at 5,000 rows** takes about 180 ms of sequential requests to the mock (25 batch reads and 25 relation reads), then about 160 ms to assemble the Hierarchy, write it to the query cache and paint. The mock answers instantly, so a real Azure DevOps load is slower.
- **Scroll and drag at 5,000 rows** cost more than at 1,000 rows (19 ms and 13 ms on runner B). The visible-rows cache still compares every node on each check, two checks per frame, as [#105](https://github.com/codeviking428/ado-planner/pull/105) noted.

These budgets keep the numbers from getting worse. They do not claim the targets are met.

## Running and updating

```sh
dbus-run-session -- .github/scripts/speed-budgets.sh       # one run, as in CI
dbus-run-session -- .github/scripts/speed-budgets.sh 3     # three runs, to recalibrate
```

The script needs `xvfb`, `mesa-vulkan-drivers`, `gnome-keyring` and `dbus-x11` (see the `speed` job in `.github/workflows/ci.yml`). Set `ADO_PLANNER_LAUNCH_LOG` to a `RUST_LOG` filter, such as `ado_client=debug`, to see where a slow launch spends its time. That prints the app's log with the time since spawn.

To change a budget, run the suite three times on a hosted runner, update the constants at the top of `launch.rs` or `speed.rs`, and update the tables above.
