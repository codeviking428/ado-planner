# Changelog

All notable changes to ADO Planner. The release PR generates each entry from
Conventional Commits; edit the wording in that PR before merging. See
[docs/RELEASING.md](docs/RELEASING.md).

## [0.4.0](https://github.com/codeviking428/ado-planner/releases/tag/v0.4.0) - 2026-11-09

### Features

- **gantt:** Pinch zoom and the no-match empty state ([#111](https://github.com/codeviking428/ado-planner/pull/111))
- **gantt:** Refuse a Dependency that would make a loop ([#121](https://github.com/codeviking428/ado-planner/pull/121))
- **core:** Plan Date cascade for Flexible, Strict and No-action ([#120](https://github.com/codeviking428/ado-planner/pull/120))
- **gantt:** Show schedule Clashes on arrows and bars, with a Show arrows toggle ([#123](https://github.com/codeviking428/ado-planner/pull/123))
- **form:** Search identities as you type in the Work Item form ([#117](https://github.com/codeviking428/ado-planner/pull/117))
- **gantt:** Run Date cascade on date changes and new Dependencies, with a Cascade mode setting ([#127](https://github.com/codeviking428/ado-planner/pull/127))
- **gantt:** Write Hierarchy Rollup to parents and cascade their successors ([#128](https://github.com/codeviking428/ado-planner/pull/128))

### Bug fixes

- **session:** Re-prompt sign-in when the Entra refresh is rejected ([#118](https://github.com/codeviking428/ado-planner/pull/118))
- **gantt:** Stop row tooltips staying behind when rows move ([#124](https://github.com/codeviking428/ado-planner/pull/124))

## [0.3.0](https://github.com/codeviking428/ado-planner/releases/tag/v0.3.0) - 2026-11-09

### Features

- **windows:** Install for all users under Program Files and drop the release console window ([#75](https://github.com/codeviking428/ado-planner/pull/75))
- Adopt the new vector app icon for windows, taskbars, and installers ([#100](https://github.com/codeviking428/ado-planner/pull/100))

### Bug fixes

- **gantt:** Show a check on the Critical path menu item while on ([#78](https://github.com/codeviking428/ado-planner/pull/78))
- **app:** Register icon assets so the theme menu button shows ([#80](https://github.com/codeviking428/ado-planner/pull/80))
- **sign-in:** Show busy state while signing in and block repeat submits ([#83](https://github.com/codeviking428/ado-planner/pull/83))
- **gantt:** Keep the visible range when the selection is off-screen on scale change ([#85](https://github.com/codeviking428/ado-planner/pull/85))
- **gantt:** Range title follows the view on every pan step ([#90](https://github.com/codeviking428/ado-planner/pull/90))
- **gantt:** Pan the same step on every scale ([#87](https://github.com/codeviking428/ado-planner/pull/87))
- **gantt:** Keep Year-view bars readable at low zoom ([#86](https://github.com/codeviking428/ado-planner/pull/86))
- **gantt:** Dismiss stale tooltips on scope and filter reloads ([#99](https://github.com/codeviking428/ado-planner/pull/99))
- **gantt:** Size the timeline to the painted planner height ([#102](https://github.com/codeviking428/ado-planner/pull/102))
- **gantt:** Keep the empty-team card border inside the planner ([#101](https://github.com/codeviking428/ado-planner/pull/101))
- **gantt:** Show one tooltip on unscheduled parent markers ([#104](https://github.com/codeviking428/ado-planner/pull/104))
- **gantt:** Stop clipping No dates pills on parent rows ([#107](https://github.com/codeviking428/ado-planner/pull/107))
- **gantt:** Keep date chips and link plates 8px from titles ([#103](https://github.com/codeviking428/ado-planner/pull/103))
- **sign-in:** Show a readable error when Azure DevOps rejects the PAT ([#79](https://github.com/codeviking428/ado-planner/pull/79))
- **gantt:** Open a create dialog before Add item creates a work item ([#82](https://github.com/codeviking428/ado-planner/pull/82))
- **gantt:** Keep the first full month label after Auto fit ([#106](https://github.com/codeviking428/ado-planner/pull/106))

### Performance

- **gantt:** Stop per-frame work over the whole Hierarchy ([#105](https://github.com/codeviking428/ado-planner/pull/105))

## [0.2.1](https://github.com/codeviking428/ado-planner/releases/tag/v0.2.1) - 2026-11-06

### Bug fixes

- **release:** Build Linux binaries on Ubuntu 22.04 so they run on glibc 2.35 ([#70](https://github.com/codeviking428/ado-planner/pull/70))

## [0.2.0](https://github.com/codeviking428/ado-planner/releases/tag/v0.2.0) - 2026-11-06

### Features

- **app:** Improve PAT setup and async feedback ([#27](https://github.com/codeviking428/ado-planner/pull/27))
- **ui:** Polish planner chrome and shorten pasted org URLs
- **dev:** Open CDP on pnpm dev and attach from agents
- Persist scope, search filters, and filter by Team member ([#31](https://github.com/codeviking428/ado-planner/pull/31))
- **planner:** Persist scope, Root types, and Team assignees ([#32](https://github.com/codeviking428/ado-planner/pull/32))
- Rust/GPUI rewrite through packager and speed budgets ([#55](https://github.com/codeviking428/ado-planner/pull/55))
- Release PR flow, signed Rust release builds, and --version ([#64](https://github.com/codeviking428/ado-planner/pull/64))

### Bug fixes

- **session:** Persist PAT Session on Windows

## [0.1.0](https://github.com/codeviking428/ado-planner/releases/tag/v0.1.0) - 2026-08-14

First public release of the Electron app: Windows NSIS installer and Linux AppImage.
