# ADO Predecessor / Successor persistence

Azure DevOps stores Predecessor/Successor as a directed, acyclic **Dependency** link (`System.LinkTypes.Dependency`), not as schedule fields. Delivery Plans then paints red/green conflict on **Target Date** (else Iteration Path end). ADO itself does **not** Date-cascade Start Date / Target Date when a predecessor moves — that write is this app's job, via the same per-Work-Item JSON Patch this app already uses.

## Verdict for Date cascade and Gantt arrows

| Fact                                                                                                                                                                                                    | Implication                                                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Link pair is `System.LinkTypes.Dependency-Forward` (Successor) / `-Reverse` (Predecessor). Topology **Dependency**, `acyclic: true`. Cycles are rejected.                                               | Gantt arrows = these relations. Do not invent a lag field.                                                                                |
| Relations live on the Work Item document. Read with `$expand=relations` (or `All`). Add/remove with JSON Patch on `/relations`. Writable attribute is **comment** (plus server `isLocked`). **No lag.** | Date cascade cannot store Monday-style lag on the ADO link.                                                                               |
| Same-org cross-project and cross-Team links are allowed. Cross-org needs `System.LinkTypes.Remote.Dependency`, which Delivery Plans does **not** draw. Excel export of a query is project-scoped.       | Gantt arrows may span Teams/projects in the org; Excel is not the source of truth.                                                        |
| PATCH of Start Date / Target Date on one Work Item does not rewrite successors. Conflict UI tells the user to change dates.                                                                             | Date cascade is client-owned. Cascade mode is this app's policy, not an ADO rule.                                                         |
| Delivery Plans: successor completing before predecessor = red. End date = Target Date, else Iteration Path finish. Target Date overrides Iteration on the plan.                                         | Match that rule for conflict paint. Unscheduled Work Items have no Target Date; Iteration is a display hint here, a conflict input there. |
| `azure-devops-node-api` `updateWorkItem` / `getWorkItem` wrap the REST PATCH/GET. No SDK batch PATCH. GET batch max **200**.                                                                            | Reuse `patchDates` + `/rev` test. Chunk reads at `BATCH_SIZE` (200).                                                                      |

## Link type `System.LinkTypes.Dependency`

System-defined work link type. Friendly names:

| End         | REST `rel`                            | CLI `ReferenceName` | Meaning                                                   |
| ----------- | ------------------------------------- | ------------------- | --------------------------------------------------------- |
| Successor   | `System.LinkTypes.Dependency-Forward` | same                | Work Item that should complete **after** the current one  |
| Predecessor | `System.LinkTypes.Dependency-Reverse` | same                | Work Item that should complete **before** the current one |

Choose **Predecessor** when linking to work that should finish first; **Successor** when linking to work that should finish later. Directional; one-to-many. Linked tasks appear as predecessor-successor links in Azure Boards.

`witadmin listlinktypes` reports:

```
Reference Name: System.LinkTypes.Dependency
Names: Successor, Predecessor
Topology: Dependency
Is Active: True
```

REST `Work Item Relation Types - List` sample for this pair:

- `topology`: `dependency`
- `acyclic`: `true`
- `directional`: `true`
- `editable`: `false` (system type)
- `usage`: `workItemLink`
- Successor `isForward: true`; Predecessor `isForward: false`; each lists the other as `oppositeEndReferenceName`

The same REST sample also sets `singleTarget: true` on this pair **and** on `System.LinkTypes.Related`. Learn documents Successor/Predecessor as one-to-many and Related as unrestricted. Do not treat that sample's `singleTarget` as a cardinality cap.

### Topology and cycle rejection

Dependency topology: directed, different names at each end, **circular relationships restricted**. Learn: "An error appears when you attempt to create circular relationships." Attribute table: `acyclic` true means the type restricts circular relationships.

You cannot customize this system type (Related, Parent-Child, Successor-Predecessor).

Cross-organization predecessor/successor is a **different** type: `System.LinkTypes.Remote.Dependency-Forward` / `-Reverse` (Produces For / Consumes From), same Entra tenant. Delivery Plans: remote link types are **not** supported for dependency lines.

Sources: [1], [2], [3], [4], [15].

## REST: read, add, remove

### Read

```
GET https://dev.azure.com/{organization}/{project}/_apis/wit/workitems/{id}?$expand=relations&api-version=7.1
```

`$expand` values: `None` (default), `Relations`, `Fields`, `Links`, `All`. Relations are omitted unless expanded. `WorkItem.relations[]` entries are `{ rel, url, attributes }`.

Bulk GET:

- `GET .../workitems?ids=...` — maximum **200** ids
- `POST .../workitemsbatch` — maximum **200** ids; body may include `$expand`

Work item URL in a relation is typically `https://dev.azure.com/{org}/_apis/wit/workItems/{id}` (collection-scoped, not project-scoped). Cross-project links in the same org still use that shape.

### Add

Media type: `application/json-patch+json`. Official sample:

```http
PATCH https://dev.azure.com/{organization}/{project}/_apis/wit/workitems/{id}?api-version=7.1

[
  { "op": "test", "path": "/rev", "value": 3 },
  {
    "op": "add",
    "path": "/relations/-",
    "value": {
      "rel": "System.LinkTypes.Dependency-forward",
      "url": "https://dev.azure.com/{org}/_apis/wit/workItems/300",
      "attributes": { "comment": "Making a new link for the dependency" }
    }
  }
]
```

`/relations/-` appends. The sample request uses lowercase `-forward`; the stored `rel` in the response is `System.LinkTypes.Dependency-Forward`. Prefer the canonical casing from Relation Types - List.

Adding a Successor on A creates the reverse Predecessor on B (one link, two ends).

### Remove / update comment

Remove by **index** in the current `relations` array:

```json
{ "op": "test", "path": "/rev", "value": 3 },
{ "op": "remove", "path": "/relations/2" }
```

Replace the comment:

```json
{ "op": "replace", "path": "/relations/2/attributes/comment", "value": "…" }
```

Always `test` `/rev` first. Relation indexes shift after a remove.

### Relation attributes: comment only, no lag

REST `WorkItemRelation.attributes` is an untyped object. Official add/update samples for Dependency set only **`comment`**. Responses also echo **`isLocked`**. There is no `lag`, `lead`, `delay`, or offset in the Work Item Tracking REST schema or samples.

Microsoft Project has lag; ADO Boards predecessor/successor does not persist it. Date cascade cannot round-trip Monday lag onto the ADO link.

Object limit: **1,000** work item links per Work Item (new links blocked at the cap).

Sources: [5], [6], [7], [8], [16].

## Cross-project / cross-Team; Excel

Same organization:

- Delivery Plans: "You can create dependencies between work items across different projects and teams within the same organization. Dependencies across organizations aren't supported."
- Related links are explicitly allowed across projects/teams; Successor/Predecessor Excel note implies cross-project links **are** creatable.

Excel restrictions (Learn, Successor-Predecessor):

- Create predecessor-successor links **only within the same project** when you plan to export to Excel.
- Cross-project predecessor-successor links are allowed in ADO, but **Excel export/import includes only items for the project that defines the query**.

Excel generally:

- A workbook's worksheets must connect to the **same project**.
- Tree lists support **tree** topology (Parent-Child), not Dependency.
- Direct-links queries import as a **flat** list: Excel does not support modifying multiple link types.
- Links and Attachments dialog is not bulk tree-edit; tree lists are for tree topology only.

Do not use Excel as the persistence path for Gantt arrows.

Sources: [1], [4], [9].

## ADO does not Date-cascade dates

Confirmed: Azure DevOps does **not** move successor Start Date / Target Date when a predecessor's dates change.

Why, from owning docs:

1. **Work Items - Update** PATCHes only the JSON Patch document for **that** Work Item. There is no documented side effect that rewrites linked Work Items' `Microsoft.VSTS.Scheduling.StartDate` / `TargetDate`.
2. **Delivery Plans** treats schedule conflict as a **view**: "Dependency conflicts occur when a successor work item is scheduled to complete before its predecessor." Resolution is **you** "Modify work item dates or iteration assignments" — not an automatic cascade.
3. Plan cards: "Work items can use Start Date and Iteration or Start Date and Target Date… Don't use both Iteration and Target Date. **Target Date always overrides the Iteration end date on the plan.**" Dates and Iteration Path are independent fields the user (or this app) writes.
4. Dragging a card on a Delivery Plan updates **that** card's dates/iteration. Docs never say successors follow.

Date cascade and Cascade mode are this app. ADO stores the Dependency and the two date fields; conflict paint is derived.

Sources: [4], [5], [10].

## Delivery Plans: lines and red/green

Prerequisite: Work Items linked with Predecessor-Successor (or a custom dependency type on-prem). Remote types: not drawn.

UI:

- Green icon: no Dependency scheduling issues
- Red icon: Dependency scheduling conflicts
- Select card edge: dependency **lines**. No conflict = black lines; conflict = red lines
- Expand both Team rows to see cross-Team lines
- Dependencies dialog lists predecessors/successors, status, and cross-project rows when present

Conflict rule (Learn):

> Dependency conflicts occur when a successor work item is scheduled to complete before its predecessor.

> Dependency end dates are determined by either the work item's **Target Date** or the **End Date** of its assigned **Iteration Path**.

> **Target Date** always overrides the **Iteration** end date on the plan.

So: compare successor **end** vs predecessor **end**. End = Target Date if set, else Iteration Path finish. Red if successor ends before predecessor ends. Green otherwise. This is **finish-to-finish style on end dates**, not start-to-start, and not Critical path.

This app's Unscheduled Work Items have empty Start Date and Target Date; Iteration Path dates are a Gantt display hint and are not PATCHed until the user schedules. Delivery Plans **will** use Iteration end for conflict when Target Date is empty. Date cascade / conflict paint must decide whether to match ADO (Iteration as end) or this app's Unscheduled rule (Iteration is not a schedule).

Sources: [4], [10].

## `azure-devops-node-api` (this app: `^15.1.1`)

`IWorkItemTrackingApi` / `WorkItemTrackingApi`:

| Method                                                                                                                | Maps to             | Notes                                                           |
| --------------------------------------------------------------------------------------------------------------------- | ------------------- | --------------------------------------------------------------- |
| `getWorkItem(id, fields?, asOf?, expand?, project?)`                                                                  | GET work item       | Pass `WorkItemExpand.Relations` (1) or `All` (4) to load links. |
| `getWorkItems(ids, fields?, asOf?, expand?, errorPolicy?, project?)`                                                  | GET list            | **Maximum 200** ids.                                            |
| `getWorkItemsBatch(workItemGetRequest, project?)`                                                                     | POST workitemsbatch | **Maximum 200** ids. `$expand` on the request object.           |
| `updateWorkItem(customHeaders, document, id, project?, validateOnly?, bypassRules?, suppressNotifications?, expand?)` | PATCH one Work Item | `document` is `JsonPatchDocument`.                              |
| `getRelationTypes()` / `getRelationType(relation)`                                                                    | Relation types      | Confirm `rel` strings.                                          |

`WorkItemExpand`: `None = 0`, `Relations = 1`, `Fields = 2`, `Links = 3`, `All = 4`.

`WorkItemRelation` extends `Link`: `{ rel?, url?, attributes?: { [key: string]: any } }`. No lag type.

There is **no** `updateWorkItems` / `$batch` method on this client. Multi-Work-Item Date cascade is N × `updateWorkItem`.

`JsonPatchDocument` is `JsonPatchOperation[]` (`add` / `remove` / `replace` / `test` / …).

Sources: [11], [12], [6], [7].

## Batch update limits

| Limit                        | Value                                                                                                                                   | Use                                                                             |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| GET / GET-batch Work Items   | **200** ids                                                                                                                             | Load relations for Date cascade; this app already chunks at `BATCH_SIZE = 200`. |
| Work Items - Update          | **1** Work Item per PATCH                                                                                                               | Each date write is one PATCH.                                                   |
| WIT `$batch`                 | Documented as wrapping multiple updates; **no numeric cap** in the 4.1/7.x batch page. `azure-devops-node-api@15.1.1` does not wrap it. | Optional later; not what this app uses.                                         |
| Rate / usage                 | **200 TSTU** per user per sliding 5 minutes; 429 / `Retry-After` / `X-RateLimit-*`                                                      | Many sequential PATCHes can delay. Honor `Retry-After`.                         |
| Links per Work Item          | **1,000**                                                                                                                               | Cap on arrows per node.                                                         |
| REST revisions per Work Item | **10,000**                                                                                                                              | Batch field changes; don't PATCH Start and Target as two revisions.             |
| WIQL results                 | **20,000**                                                                                                                              | This app already caps hierarchy WIQL at `WIQL_RESULT_CAP`.                      |

Microsoft: batch changes; don't update one field at a time; handle failures. `$batch` failed items do not stop later items in that batch (REST batch page).

Date cascade should: one PATCH per affected Work Item with **both** dates + `test /rev`; chunk any GET of relations at 200; backoff on 429.

Sources: [6], [7], [8], [13], [14], [16], [17].

## What this app already does (`src/main`) — Date cascade must reuse

Today there are **no** Dependency reads or writes. Hierarchy GET uses a field list (`HIERARCHY_FIELDS`) and does not `$expand` relations. Form GET also omits expand. Gantt drag PATCHes only Start Date and Target Date on **that** Work Item.

Reuse this path:

1. **`AdoClient.patchDates` / `RestAdoClient.patchDates`** — the only Work Item write. `saveForm` already delegates to `patchDates`.
2. **`buildStartTargetPatch`** (`src/shared/dates.ts`) — `{ test /rev, add /fields/Microsoft.VSTS.Scheduling.StartDate, add /fields/Microsoft.VSTS.Scheduling.TargetDate }` with `isoDateOnly` (`YYYY-MM-DDT00:00:00Z`).
3. **`toVssPatch`** — maps `test|add|remove|replace` to `Operation.*` for the node client.
4. **IPC `ado:patch-dates`** — `patchDatesSchema` (`org`, `project`, `id`, `rev`, `startDate`, `targetDate`); handler builds the document; renderer optimistic-updates then rolls back on failure.
5. **`updateWorkItem(undefined, patch, id, project, false)`** — `false` is `validateOnly`, not expand. Do not pass `true` (dry-run). Leave `bypassRules` unset.
6. **Read chunking** — `chunkIds(ids, BATCH_SIZE)` with `BATCH_SIZE = 200` and `getWorkItemsBatch` / REST `workitemsbatch` (`errorPolicy` Omit / `2`). Add `$expand: Relations` (or a WorkItemLinks WIQL) when loading arrows; do not raise the chunk size.
7. **`START_DATE_FIELD` / `TARGET_DATE_FIELD`** — `Microsoft.VSTS.Scheduling.StartDate` / `TargetDate`.

Gaps Date cascade must add (not present today):

- Load `relations` (`getWorkItem`/`getWorkItemsBatch` with `WorkItemExpand.Relations`, or WIQL `FROM WorkItemLinks` on `System.LinkTypes.Dependency-*`).
- Walk Successor/Predecessor for Cascade mode / Critical path. ADO will not walk it.
- PATCH many Work Items: loop existing `patchDates` (new `rev` after each success). No SDK batch write.
- `RestAdoClient` sends `Content-Type: application/json`. Official PATCH media type is `application/json-patch+json`. The node client sets the patch type. Prefer the official media type on the REST path.

Sources: [5], [11], [12], and this repo (`src/main/ado-client.ts`, `src/main/ado-rest.ts`, `src/main/ipc.ts`, `src/shared/dates.ts`, `src/shared/types.ts`, `src/shared/hierarchy.ts`).

## Sources

1. Link types reference (Successor/Predecessor, topology, cycles, Excel, remote types) — https://learn.microsoft.com/en-us/azure/devops/boards/queries/link-type-reference
2. Link type topologies (Dependency vs Tree vs Network; acyclic) — https://learn.microsoft.com/en-us/previous-versions/azure/devops/reference/xml/link-type-element-reference
3. Work Item Relation Types - List (Forward/Reverse, `acyclic`, `topology`) — https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-item-relation-types/list?view=azure-devops-rest-7.1
4. Track dependencies in Delivery Plans (cross-project/Team, red/green, Target Date vs Iteration end) — https://learn.microsoft.com/en-us/azure/devops/boards/plans/track-dependencies
5. Work Items - Update (JSON Patch, add/remove relation, comment, `application/json-patch+json`) — https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/update?view=azure-devops-rest-7.1
6. Work Items - Get (`$expand=relations`) — https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/get-work-item?view=azure-devops-rest-7.1
7. Work Items - Get Work Items Batch (max 200, `$expand`) — https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/get-work-items-batch?view=azure-devops-rest-7.1
8. Work Items - List (max 200 ids) — https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/list?view=azure-devops-rest-7.1
9. Bulk add or modify work items with Excel (same project, tree vs direct links) — https://learn.microsoft.com/en-us/azure/devops/boards/backlogs/office/bulk-add-modify-work-items-excel
10. Use team delivery plans (Target Date overrides Iteration end; drag dates) — https://learn.microsoft.com/en-us/azure/devops/boards/plans/review-team-plans
11. `azure-devops-node-api@15.1.1` `WorkItemTrackingApi.d.ts` (`getWorkItem`, `updateWorkItem`, `getWorkItemsBatch`) — https://cdn.jsdelivr.net/npm/azure-devops-node-api@15.1.1/WorkItemTrackingApi.d.ts
12. `azure-devops-node-api@15.1.1` `WorkItemTrackingInterfaces.d.ts` (`WorkItemExpand`, `WorkItemRelation`) — https://cdn.jsdelivr.net/npm/azure-devops-node-api@15.1.1/interfaces/WorkItemTrackingInterfaces.d.ts
13. Rate and usage limits (200 TSTU / 5 min, 429, `Retry-After`) — https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits
14. Integration best practices (batch writes, revision cap, link cap) — https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/integration-bestpractices
15. Add a link to a Work Item (Predecessor/Successor UI) — https://learn.microsoft.com/en-us/azure/devops/boards/backlogs/add-link
16. Work tracking object limits (1,000 links, 10,000 REST revisions, 20,000 query results) — https://learn.microsoft.com/en-us/azure/devops/organizations/settings/work/object-limits
17. Work item batch updates (`PATCH .../_apis/wit/$batch`, API 4.1; no SDK wrapper) — https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/workitembatchupdate?view=azure-devops-rest-7.1
