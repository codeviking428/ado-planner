# Refuse Dependency loops in the planner, and judge Clashes on Start and Target Dates only

Azure DevOps rejects a looping Predecessor/Successor link, but only after a round trip, and its error names no path. Delivery Plans paints a conflict when a successor's end (Target Date, else Iteration Path finish) comes before its predecessor's end. Monday paints overlap in red and moves dependents by Cascade mode. We check loops in the planner before we PATCH, and we call a Dependency a **Clash** when the successor starts before the predecessor's Target Date, using only Start Date and Target Date. Iteration Path dates never enter the rule, because on this Gantt they are a display hint, not a schedule (see **Unscheduled** in `CONTEXT.md`).

This locks [How do we show cycles and schedule clashes?](https://github.com/codeviking428/ado-planner/issues/40) and the fog left on [Monday Gantt parity spec](https://github.com/codeviking428/ado-planner/issues/33). It builds on [#37](https://github.com/codeviking428/ado-planner/issues/37) (Date cascade rules), [#38](https://github.com/codeviking428/ado-planner/issues/38) (Dependency CRUD) and [#39](https://github.com/codeviking428/ado-planner/issues/39) (Critical path).

## Decisions

**Loops.** Before the PATCH, walk Predecessors back from the new predecessor over every loaded Work Item, bars or not. If the walk reaches the new successor, refuse the link. While the connector hovers such a bar, the bar shows the refused state (destructive outline, not-allowed cursor) and a tooltip naming the loop, `Would loop: #12 → #34 → #56 → #12`, cut to five ids and `…`. A drop there sends no PATCH and raises a warning toast with the same text, the way `Link already exists` does today. Undo or redo that would re-add a looping link gets the same check. A loop through Work Items outside the forest still reaches Azure DevOps; keep today's revert and error toast for it.

**Clash rule.** A Dependency clashes when both ends have bars drawn from their own fields and the successor's Start Date is before the predecessor's Target Date. Equal dates do not clash, matching #37's Flexible rule (Start Date ≥ predecessor Target Date). A Milestone counts its Target Date as its start and its end. These never clash: a predecessor with no Target Date, an Unscheduled Work Item (even one showing Iteration or Rollup hint dates), a type without date fields, and an off-forest end.

**Clash at rest.** One look in every Cascade mode, since each can leave clashes: Flexible after edits made in Azure DevOps, Strict when the user drops a successor into overlap, No-action always. The arrow takes the Flavor's `warning` token. The successor bar gets a warning `!` badge at its start edge, with one tooltip line per clashing predecessor, `Starts Jul 9 · #12 finishes Jul 10`. Clash arrows draw even when Show arrows is off, as Critical path arrows do. On the Critical path, a clashing arrow keeps the path's width and takes the clash colour. No green "all clear" icon: a missing badge says that. The planner never fixes a loaded clash on its own; dates change only after a user gesture.

**Clash during a drag.** Paint what the drop will leave. While a bar is dragged, resized or nudged, re-run the Clash rule on the tentative dates, after the Date cascade plan once that exists, and update arrows and badges live. Cancel restores them. Under Flexible and Strict the drag chip adds `· moves N`; successors move on drop, not during the drag.

**Write failures.** A Date cascade write that fails for good reverts that row (#37) and shows the same `!` badge, with `Couldn't move: <reason>`. One badge per bar; its tooltip lists every reason. The failure badge lasts until the next good write to that Work Item or a reload.

**Cascade modes.** Flexible, Strict and No-action as #37 locked them, default Flexible, stored on this machine with the other planner prefs. Dependency delete never cascades.

**Milestones.** A Milestone is a Target Date with no Start Date, drawn as a diamond; that already ships. Start Date equal to Target Date stays a one-day bar. Azure DevOps dates are whole, inclusive days, so equal dates mean one day of work, and #37 gives an Unscheduled successor exactly that bar; drawing it as a diamond would make every such cascade look like a new Milestone. A Date cascade moves a Milestone's Target Date and leaves Start Date empty.

**Controls.** All three live in the Gantt toolbar's `⋯` menu, which already holds Critical path:

| Item                                        | Kind             | Default  | Stored          |
| ------------------------------------------- | ---------------- | -------- | --------------- |
| Show arrows                                 | `✓` toggle       | on       | planner prefs   |
| Critical path                               | `✓` toggle (#78) | off      | no (as shipped) |
| Date cascade: Flexible / Strict / No-action | one `✓` of three | Flexible | planner prefs   |

Monday keeps these off the chart too: Show Dependencies in Visual settings, the mode in the Dependency Column settings, Critical path behind the settings cog. Delivery Plans keeps them in plan settings. The toolbar has no room for three more buttons.

## Considered Options

- **Let Azure DevOps reject loops.** That is today's behaviour. Rejected: the user waits for a round trip, sees an optimistic arrow vanish, and learns nothing about the path.
- **Follow Delivery Plans' rule** (successor end before predecessor end, Iteration Path finish when Target Date is empty). Rejected: it reads Iteration dates as a schedule, which this Gantt does not, and it flags Unscheduled Work Items the user cannot drag. Among Work Items with bars our rule is stricter: every successor Delivery Plans paints red starts before its predecessor ends, so it clashes here too. Delivery Plans can still show red on Unscheduled or off-forest work that we leave plain.
- **Red for Clashes**, as Delivery Plans and Monday do. Rejected: red already marks the Critical path (#39).
- **A toolbar Clash count with next-clash navigation.** Deferred: the badge and arrow cover the need; add the count if users miss off-screen Clashes.
- **Move successors live during a drag.** Deferred: more paint per frame across the chain, and Monday moves them on drop.

## Consequences

- A Clash is derived, like the Critical path. Nothing new is stored in Azure DevOps.
- Delivery Plans and the planner can disagree on the same link. When they do, the planner is right about its own fields, and the Iteration Path is the gap.
