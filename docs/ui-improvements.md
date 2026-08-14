# UI improvements

Running list for bringing ADO Planner to professional-grade (Monday-class) polish. Check items off as they land; add follow-ups from Playwright screenshots.

## Layout and hierarchy

- [x] Collapse the two checkbox rows into compact Type / State filter menus
- [x] Give the tree pane enough width so Work Item, Type, and dates are not clipped
- [x] Put brand, scope, filters, and account on one scannable toolbar
- [x] Move Flavor out of the primary filter row into an appearance menu
- [x] Wrap the Gantt in a contained board surface (radius, border, canvas contrast)
- [x] Sign-in: labeled fields in a card, not a loose stack of inputs

## Readability

- [x] Stop wrapping Type names; show a color badge that truncates cleanly
- [x] Show Unscheduled vs scheduled dates as two lines instead of one clipped sentence
- [x] Native `title` tooltips on truncated Work Item titles and date cells
- [x] Bump Gantt type from `text-xs` to a readable 13px
- [x] Widen the Work Item name column so IDs and titles can sit side by side
- [x] Scope selects show human labels (`All`, `Anyone`), never raw values like `__none__`

## Controls and interaction

- [x] Replace native `<select>` with the existing Select component
- [x] Filter menus: checked = visible (not “hide”), with Show all / Hide all
- [x] Keep `#org` / `#project` / `#team` / `#iteration` and loading test ids
- [x] Validate with Playwright: scope switching, filters, unclipped titles

## Follow-ups from screenshots

- [x] Iteration showed `__none__` when empty — SelectValue now maps to the placeholder
- [x] Assignee showed lowercase `anyone` — mapped to `Anyone`
- [x] Unscheduled second line dropped the “iteration/rollup” prefix so the dates fit
- [x] Filters group labeled to match Org / Project / Team
- [x] Filter menu: checked types stay visible; Show all / Hide all work
- [ ] Tree column headers still sit a few pixels off the cell text (Gantt chrome)
- [ ] Month-scale bars for short Work Items are easy to miss; consider a week default later
