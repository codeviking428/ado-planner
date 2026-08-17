# Monday Gantt and Dependencies

Monday's board Gantt **visualizes** Dependencies as arrows and **runs Date cascade** when you drag a Gantt bar. It does **not** document drawing or deleting arrows on the chart. Create, edit, type (FS/SS/FF/SF), and lead/lag live on the Dependency Column. Cascade mode (Flexible / Strict / No-action) is a column setting, not a Gantt control. Critical path is a red highlight of the longest duration over currently viewed items.

ADO Planner maps: Monday item → Work Item; bar → Gantt bar; Dependency Column cell → Predecessor list; Flexible/Strict/No-action → Cascade mode; blank dates → Unscheduled. ADO Planner Dependencies are Finish-to-start only; Monday's four types are facts about Monday, not a product requirement.

## Anatomy of the Gantt

The board Gantt (board view or Dashboard widget) is a grid: items listed vertically against a horizontal calendar. Left panel: item names and dates. Right panel: a Gantt bar per item. Arrow lines between bars show Dependencies. Hovering an item highlights the entire row. [[1]](#1)

A Timeline Column or Date Column supplies the dates the bars use. People and Status columns are optional. The Dependency Column is optional but is how you **set** which items depend on which, if the project has Dependencies. [[1]]

The Timeline Column article describes the same layout: items and dates on the left, Gantt bars on the right, connecting lines for Dependencies if any exist. You can open Gantt from a Timeline cell via "View in Gantt". [[6]]

## Creating, editing, deleting Dependencies

### Dependency Column (source of truth)

One Dependency Column per board. Adding it opens settings: Cascade mode (Flexible / Strict / No-action) and which time column the links constrain (Date or Timeline). Subitem summary columns are not supported there. [[2]]

Click a cell → sidebar to pick any item on the board. Search by name, or (Enterprise project boards, beta) by Item ID. [[2]]

The cell stores items this Work Item **depends on** (Predecessors). The monday.com API names them `linked_item_ids` / `linked_items`. Setting the column **replaces** the whole list. Clear with `null` or `{}`. Relationship type (FS/SS/FF/SF) is **not** exposed by the API. Cycle detection is **not** enforced by the API. [[9]] [[10]]

### Board Gantt

Official board Gantt docs tell you to set Dependencies on the Dependency Column so the Gantt knows "which items must be finished before other tasks can get started." They do **not** document click-drag from a bar handle to create a link, or click-an-arrow to delete one. [[1]]

Click any Gantt bar to open the item pop-up card. The card edits **all** columns, so Dependency Column CRUD is available from the Gantt only by opening that card (or leaving the view). Changes write through to the Main Table. [[1]] [[8]]

### WorkCanvas Gantt (different product)

WorkCanvas's Gantt **element** (not the board view) does document arrow CRUD: hover the side of a task → blue "+" → drag to another task; click the line → Trash to delete. Shifting the first task then shifts the dependent task. Do not copy this as board-Gantt behavior. [[7]]

## Arrows: drawing, hover, show/hide

**Draw / delete on the board Gantt.** Not documented. "Show Dependencies" in **Visual settings** only hides or shows the arrows; it does not remove the Dependency. [[1]]

**Arrow geometry.** "The dependency line connects a task with another task that is immediately dependent upon its completion, with the arrow pointing to the dependent task." That wording is Finish-to-start flavored. Monday still has four types per link (below); the Gantt article does not say arrows change shape per type. [[1]]

**Hover.** Board Gantt: hover highlights the whole row. [[1]] Cross-project Dependencies (Enterprise project boards): hover a task for board name, owner, status, timeline, type, and overlap; Gantt draws conflict-free cross-project links in black and conflicts in red, with a red exclamation for overlapping days. [[5]]

## Four types and three Cascade modes when dragging bars

### Types (per Dependency)

Set per relationship while creating/editing the link. Gradually rolling out. [[2]] [[4]]

| Type                  | Rule (Monday's wording)                                      | Column UI                                       |
| --------------------- | ------------------------------------------------------------ | ----------------------------------------------- |
| Finish-to-start (FS)  | Only once Task A is finished, Task B can start. Most common. | Default; **no** visual indication in the column |
| Start-to-start (SS)   | Only once Task A starts can Task B also begin                | Shown                                           |
| Finish-to-finish (FF) | Only once Task A is finished can Task B also be finished     | Shown                                           |
| Start-to-finish (SF)  | Only once Task A has started, Task B is able to finish       | Shown                                           |

ADO Planner Dependencies are Finish-to-start only. [[CONTEXT.md](../../CONTEXT.md)]

### Cascade modes (per column, not per link)

Chosen when the Dependency Column is added. They control how items update when the items they depend on change. [[2]]

| Mode          | Date cascade                                                                                                                                                                              |
| ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Flexible**  | No overlap between dependent dates. If you change a date and there is **no** overlap with dependents, they stay put. If there **is** overlap, dependents shift until the overlap is gone. |
| **Strict**    | Dependents change **in the exact same way** as the Predecessor (optimized schedule, with or without predefined gaps). Lead/lag only in this mode.                                         |
| **No-action** | No automatic shift, even on a clash. Relationships still show in the column and as arrows on the Gantt Widget.                                                                            |

Batch-edit of Date/Timeline with Dependencies is unsupported except in No-action. [[2]]

The API stores this as column `settings.dependency_mode`: `flexible` | `strict` | `no_action`. It only has effect if the board also has a Date or Timeline column. [[9]] [[10]]

**API vs support conflict.** Support: modes fire when you **change dates/timelines** (including Gantt/Timeline drag). [[2]] Developer docs: modes fire when a Dependency is **completed / marked done / status changes**. [[9]] [[10]] For Gantt Date cascade, follow support.

### Dragging Gantt bars

With the Dependency Column set up, click a Gantt bar and drag it by the delay; **all** dependent tasks shift by that many days. The same Date cascade runs when you edit the Date/Timeline column. Changes write to the Main Table. The same sentence covers Timeline View drag. [[1]] [[2]]

Official copy does **not** spell out a different drag gesture per FS/SS/FF/SF or per Flexible/Strict/No-action. Mode still applies: Strict moves dependents the same amount; Flexible only if the drag creates overlap; No-action leaves dates (arrows remain). [[2]]

**Unscheduled.** Filling the Dependency Column does **not** create dates. Date cascade only **shifts existing** Date/Timeline values. Blank = Unscheduled; use a placeholder date or the Duration (Timeline + Numeric) column so there is something to move. [[2]] [[6]]

## Lead / lag visualization

Strict Cascade mode only. Edited on the Dependency Column, not on the Gantt. Positive number = **lag** (wait between Predecessor and Successor). Negative number = **lead** (overlap / parallel start). [[2]]

On Timeline or Gantt: lag is a **gap** between the dependent Gantt bars; lead is an **overlap**. monday AI work platform only (not monday CRM / monday dev); gradual rollout. [[2]]

## Critical path

Monday: "the longest duration of tasks in your project." Used to see which tasks must finish on time for the project to finish on schedule, and to separate critical vs non-critical tasks and Dependencies. [[3]]

ADO Planner: longest Dependency chain **by dates**; a derived highlight, not a stored field. Monday's wording is duration-of-tasks, not "longest Dependency chain," and the path is computed over **all viewed items**, including multiple Date/Timeline columns and, on a Dashboard widget, items from multiple boards. [[3]] [[CONTEXT.md](../../CONTEXT.md)]

**Drawn:** Settings cog → Critical Path → "Show critical path." The project's timeline is **highlighted in red**. [[3]]

Plan notes conflict: the Gantt article says milestone **and** critical path are Pro/Enterprise; the Critical Path article still calls it a **trial** that may require payment later. [[1]] [[3]]

## Brief: out-of-scope surfaces

**Milestones.** Diamond on Timeline and Gantt. Enable "Show 'Set as milestone'" on the Timeline Column (already on for project boards). Set from the Gantt by opening the bar's pop-up → Timeline Column. Pro/Enterprise. [[1]] [[4]]

**Baselines.** Snapshot of the current Timeline/Date column: gray locked bars. On-track items green; delayed/extended items **and their dependents** red. Settings list snapshots; show/hide; compare. Creating a baseline **duplicates** the time column (locked original vs live) and adds a Formula Column for day delta — deleting those columns deletes the baseline. Subitems can appear via "Choose Timeline columns." Snapshots are not editable. Pro/Enterprise. [[1]] [[11]]

**Subitems.** Supported on Gantt board view **and** widget via "Choose Timeline columns." [[1]] Not as dependency-settings time columns (no subitem summaries). [[2]] Enterprise batch Dependencies: selecting mixed Hierarchy levels in Strict mode links at the **highest** level (subitems roll up); parent↔subitem batch links are blocked on regular project boards — select one level to link peers. [[2]]

**Widget vs board view.** Same Gantt product. Board view is one board. Widget on a Dashboard / Dashboard View can connect multiple boards; Critical path then covers whichever Timeline columns you show. [[1]] [[3]] Export, Visual settings (including Show Dependencies), and subitem column picker exist on both. [[1]]

**PDF export.** Export arrow → PDF or Excel. PDF is a snapshot of the current Gantt: expand the left panel for names/dates, collapse for bars-only; zoomed to the full horizontal timeline and a vertical list of all tasks. Cap: 10 years of timeline and 640 items, else an error. [[1]]

## Other facts for Date cascade and Dependency CRUD

- **Gantt is Standard+; Dependencies are Pro+.** A Standard Gantt can show bars without a Dependency Column. [[1]] [[4]]
- **Weekends.** Gantt Visual settings can hide weekends (display only). Timeline/Date column "Include weekends" changes scheduling math. Admin "Hide Weekends" is required for Dependencies to skip weekends. Weekend skip works on Timeline with Dependencies, **not** on Date columns when using dependency automation recipes. [[1]] [[2]]
- **Duration column.** Timeline + Numeric combo; typing days fills an Unscheduled Timeline from today, or adjusts the end date if a range exists. Bidirectional with the Timeline. Built into project boards. [[2]]
- **Batch Dependencies** (Enterprise project boards, ≤50 items): multi-select → Dependencies; links in selection order. Does not fill blank dates. In Strict mode with existing timelines, bars are laid end-to-end. Undo via toast or Activity log. [[2]]
- **Cross-project Dependencies** (Enterprise, gradual): "See all projects" in the Dependent On column; pick FS/SS/FF/SF. Not on private/shareable projects or closed workspaces. Gantt is the cross-board timeline; conflicts are red. [[5]]
- **API.** `allowMultipleItems` on column settings. Types and lead/lag are UI-only. [[9]] [[10]]

## Sources

Official monday.com Help Center (Zendesk) and developer.monday.com. Help Center `updated_at` from `https://support.monday.com/api/v2/help_center/en-us/articles/<id>.json`.

1. [The Gantt Chart View and Widget](https://support.monday.com/hc/en-us/articles/360015643840-The-Gantt-Chart-View-and-Widget) — updated 2026-07-28. Anatomy, Visual settings / Show Dependencies, bar-drag Date cascade, widget vs board, subitems, milestones, baseline pointer, critical path pointer, PDF limits.
2. [Dependencies on monday.com](https://support.monday.com/hc/en-us/articles/360007402599-Dependencies-on-monday-com) — updated 2026-07-28. Column CRUD, one column per board, Cascade modes, four types, lead/lag, Gantt/Timeline drag, Unscheduled, Duration, batch Dependencies, weekends.
3. [Critical Path for the Gantt Chart](https://support.monday.com/hc/en-us/articles/4420037448850-Critical-Path-for-the-Gantt-Chart) — updated 2026-07-30. Longest duration; red highlight; computed over viewed items / multiple columns / multi-board widgets; trial note.
4. [Project management with monday.com](https://support.monday.com/hc/en-us/articles/360014437599-Project-management-with-monday-com). FS/SS/FF/SF + Flexible/Strict/No action; Dependencies Pro+; Gantt arrows; Gantt Standard+; milestone diamonds.
5. [Cross-Project Dependencies](https://support.monday.com/hc/en-us/articles/24601183683474-Cross-Project-Dependencies) — updated 2026-08-12. Column + type picker; Gantt black/red; hover details.
6. [The Timeline Column](https://support.monday.com/hc/en-us/articles/115005333969-The-Timeline-Column) — updated 2026-06-03. Gantt from Timeline cell; left list + bars + dependency lines; template/Unscheduled workaround.
7. [WorkCanvas Gantt element](https://support.monday.com/hc/en-us/articles/22569805470610-WorkCanvas-Gantt-element). Arrow draw (`+`) and delete (Trash) — **not** the board Gantt.
8. [The item pop-up card](https://support.monday.com/hc/en-us/articles/360001568919-The-item-pop-up-card). Gantt opens the card; all columns editable.
9. [Working with the dependency column](https://developer.monday.com/api-reference/docs/working-with-dependency-column) — developer.monday.com, updated 2026-03-31. `linked_item_ids`, replace-on-write, modes, types not in API, no cycle detection.
10. [Dependency column reference](https://developer.monday.com/api-reference/reference/dependency) — developer.monday.com, updated 2026-03-31. `DependencyValue`, `settings.dependency_mode`, `allowMultipleItems`.
11. [The Gantt Baseline](https://support.monday.com/hc/en-us/articles/360020978159-The-Gantt-Baseline) — updated 2025-07-02. Snapshot, colors, duplicated time column, Formula Column, subitems.
