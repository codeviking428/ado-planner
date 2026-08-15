import type { OverlayFilter, WorkItemNode } from './types'

export function applyOverlays(nodes: WorkItemNode[], filter: OverlayFilter): WorkItemNode[] {
  const byId = new Map(nodes.map((node) => [node.id, node]))
  const matching = new Set<number>()

  for (const node of nodes) {
    if (matchesOverlay(node, filter, byId)) {
      matching.add(node.id)
    }
  }

  if (matching.size === 0) {
    return []
  }

  const visible = new Set(matching)
  const roots = filter.rootTypes

  for (const id of matching) {
    let current = byId.get(id)
    while (current?.parentId) {
      if (roots && roots.length > 0 && roots.includes(current.type)) {
        break
      }
      visible.add(current.parentId)
      current = byId.get(current.parentId)
    }
  }

  return nodes
    .filter((node) => visible.has(node.id))
    .map((node) =>
      node.parentId != null && !visible.has(node.parentId) ? { ...node, parentId: null } : node
    )
}

function isUnderRootType(
  node: WorkItemNode,
  byId: Map<number, WorkItemNode>,
  rootTypes: string[]
): boolean {
  let current: WorkItemNode | undefined = node
  while (current) {
    if (rootTypes.includes(current.type)) {
      return true
    }
    current = current.parentId == null ? undefined : byId.get(current.parentId)
  }
  return false
}

function matchesOverlay(
  node: WorkItemNode,
  filter: OverlayFilter,
  byId: Map<number, WorkItemNode>
): boolean {
  if (filter.rootTypes !== undefined && filter.rootTypes !== null) {
    if (filter.rootTypes.length === 0 || !isUnderRootType(node, byId, filter.rootTypes)) {
      return false
    }
  }
  if (filter.types !== null && !filter.types.includes(node.type)) {
    return false
  }
  if (filter.states !== null && !filter.states.includes(node.state)) {
    return false
  }
  if (filter.iterationPath && node.iterationPath !== filter.iterationPath) {
    return false
  }
  if (filter.assignee === 'anyone') {
    return true
  }
  if (filter.assignee === 'unassigned') {
    return !node.assignedTo
  }
  const wanted =
    filter.assignee === 'me'
      ? filter.currentUserUniqueName?.toLowerCase()
      : filter.assignee.toLowerCase()
  if (!wanted || node.assignedTo?.uniqueName.toLowerCase() !== wanted) {
    return false
  }
  return true
}
