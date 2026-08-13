# ADO Planner

A Monday-lite Electron app for viewing an Azure DevOps Team's Work Item Hierarchy on a Gantt.

## Language

**Work Item**:
A unit of planned work in Azure DevOps — Epic, Feature, User Story, Product Backlog Item, Task, Bug, and process-specific types.
_Avoid_: ticket, card, item (bare)

**Hierarchy**:
The parent/child tree of Work Items from Epic down to Task inside a Team's area paths.
_Avoid_: board columns, backlog level as the view

**Team**:
An Azure DevOps project team whose area paths bound the Hierarchy shown on the Gantt.
_Avoid_: board (when meaning the Kanban board)

**Gantt bar**:
The timeline representation of a Work Item's Start Date and Target Date.

**Session**:
The securely stored Entra work/school login that lets the app call Azure DevOps without signing in every launch.
_Avoid_: personal Microsoft account, MSA, PAT as the primary login

**Flavor**:
A Catppuccin palette: Latte, Frappé, Macchiato, or Mocha.

**Unscheduled**:
A Work Item whose Start Date and Target Date are empty. Iteration Path dates may be shown as a display hint; they are not a schedule until the user drags or edits dates.
