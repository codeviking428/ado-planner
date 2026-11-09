# Planner frame times

CPU time to build one planner frame for large synthetic Hierarchies, before and after [#105](https://github.com/codeviking428/ado-planner/pull/105) stopped the per-frame work over every node ([#76](https://github.com/codeviking428/ado-planner/issues/76)). No budget is set here; enforcing budgets in CI is [#53](https://github.com/codeviking428/ado-planner/issues/53).

## How to measure

```sh
ADO_PLANNER_SPEED_TESTS=1 xvfb-run -a cargo test --release -p ado-planner --test speed -- --nocapture planner_frame_times
```

`planner_frame_times` in `crates/app/tests/speed.rs` prints the table rows below. Without `ADO_PLANNER_SPEED_TESTS` it returns at once; `frame_time_harness_paints_scrolls_and_filters` runs the same harness on 200 rows in every CI job, so the benchmark keeps building and working.

- **Dataset:** `ado_shaped_hierarchy` in the same file. Epic → 4 Features → 5 User Stories → 4 Tasks, cut at 1,000, 5,000 and 10,000 rows. Epics and Features are undated, so their bars are Rollups; one Story in four and one Task in three are undated too. Every node carries the Hierarchy batch fields, sibling Stories chain as Dependencies, and `hierarchy` is set as after a Team load.
- **Frames:** a real `PlannerView` in a GPUI test window at 1280×800. One first paint, then medians of 40 wheel-scroll frames (36 px each), 40 row-hover frames, 10 Epic collapse/expand frames and 10 State-filter toggles.
- **What it measures:** the GPUI test platform does layout and paint bookkeeping but no GPU work. These are CPU frame-build times on Linux, not on-screen latency on Windows.

## Results

Linux cloud VM, 8 vCPU Intel Xeon, 47 GiB RAM, rustc 1.99.1, release profile, under Xvfb, on 2026-11-09. Both columns ran the same harness on the same machine with nothing else running:

- **Before:** `3e4ed7f`, the commit #105 was measured against, with this harness copied in.
- **After:** `05b3e6e`, which is v0.3.0 plus the row-tooltip fix from #76. The cell shows two runs taken minutes apart; they differ by up to a quarter.

|   Rows | Frame       |   Before |           After |
| -----: | ----------- | -------: | --------------: |
|  1,000 | first paint |  23.7 ms |    8.4 / 6.5 ms |
|  1,000 | scroll      |  81.1 ms |  25.1 / 19.3 ms |
|  1,000 | hover       |  47.3 ms |  14.9 / 12.1 ms |
|  1,000 | collapse    |  73.9 ms |  19.6 / 19.9 ms |
|  1,000 | filter      |  71.9 ms |  23.6 / 24.1 ms |
|  5,000 | first paint | 270.6 ms |   9.0 / 10.6 ms |
|  5,000 | scroll      | 843.8 ms |  27.4 / 27.9 ms |
|  5,000 | hover       | 536.9 ms |  16.7 / 16.8 ms |
|  5,000 | collapse    | 810.7 ms |  27.9 / 33.6 ms |
|  5,000 | filter      | 817.0 ms |  54.0 / 67.6 ms |
| 10,000 | first paint |    1.2 s |  15.7 / 15.7 ms |
| 10,000 | scroll      |    3.2 s |  35.2 / 43.3 ms |
| 10,000 | hover       |    2.1 s |  19.8 / 23.9 ms |
| 10,000 | collapse    |    2.9 s |  40.0 / 49.4 ms |
| 10,000 | filter      |    3.0 s | 96.0 / 110.2 ms |

#105 measured the same harness on another VM of the same shape and got the same picture: 2.59 s → 30.8 ms per scroll frame at 10,000 rows. Its table is in the PR description.

## Reading the numbers

- Rows were windowed before #105 too: the tree and the timeline paint the rows on screen plus 2 overscan. What grew faster than the row count was data work over every node on each frame, such as cloning the visible nodes and rebuilding Rollups. After #105 a frame does that work for the windowed rows only.
- Some cost still grows with N. The visible-rows cache compares every node on each check, and a filter change rebuilds every visible node once.
- #105 profiled about 16 ms of each scroll frame as GPUI laying out and painting the whole window, the same at every row count.
