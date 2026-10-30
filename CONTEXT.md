# ADO Planner

A Monday-lite Electron app for viewing an Azure DevOps Team's Work Item Hierarchy on a Gantt.

## Language

**Work Item**:
A unit of planned work in Azure DevOps — Epic, Feature, User Story, Product Backlog Item, Task, Bug, and process-specific types.
_Avoid_: ticket, card, item (bare)

**Hierarchy**:
The parent/child tree of Work Items from Epic down to Task inside a Team's area paths.
_Avoid_: board columns, backlog level as the view

**Root type**:
A Work Item type that may appear at the top of the Hierarchy. Descendants of any type still nest under those roots. A Work Item with no ancestor of a Root type is not shown.
_Avoid_: top-level ticket, backlog level, sidebar root, Unparented bucket

**Team**:
An Azure DevOps project team whose area paths bound the Hierarchy shown on the Gantt.
_Avoid_: board (when meaning the Kanban board)

**Gantt bar**:
The timeline representation of a Work Item's Start Date and Target Date.

**Dependency**:
A Predecessor/Successor link between two Work Items that constrains schedule. Finish-to-start only.
_Avoid_: Hierarchy, parent/child, related (bare)

**Date cascade**:
Moving a Work Item's Start Date and Target Date because a Dependency requires it.
_Avoid_: auto-shift, ripple, Rollup

**Cascade mode**:
How Date cascade behaves: Flexible, Strict, or No-action. A local planner preference; default Flexible.
_Avoid_: dependency mode

**Rollup**:
The parent Work Item's Start Date and Target Date: min Start Date and max Target Date over descendants that have those fields.
_Avoid_: Date cascade, summary bar (when meaning the persisted dates)

**Critical path**:
The Dependency chain in the current forest whose last Target Date is latest. A derived highlight, not a stored field. Only Work Items with Start Date and Target Date are vertices.
_Avoid_: critical-path field, Hierarchy as a path

**Nudge**:
Moving the selected Gantt bar's Start Date, Target Date, or both by one day from the keyboard. Rapid nudges save as one change.
_Avoid_: shift, bump

**Session**:
The securely stored login that lets the app call Azure DevOps without signing in every launch — Entra work/school SSO when a public-client ID is configured, otherwise a stored personal access token.
_Avoid_: personal Microsoft account, MSA

**Flavor**:
A Catppuccin palette: Latte, Frappé, Macchiato, or Mocha.

**HTML field**:
A Work Item field whose value is HTML, such as Description. The planner edits it only when it is a **Plain HTML field**; otherwise it is read-only and edited in Azure DevOps.
_Avoid_: rich text field, Markdown field

**Plain HTML field**:
An HTML field whose content survives conversion to Markdown and back unchanged, so editing it in the planner loses nothing.
_Avoid_: simple description, lossless field

**Unscheduled**:
A Work Item whose Start Date and Target Date are empty. Iteration Path dates may be shown as a display hint; they are not a schedule until the user drags or edits dates.
