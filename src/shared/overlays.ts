import type { OverlayFilter, WorkItemNode } from './types'

export function applyOverlays(nodes: WorkItemNode[], filter: OverlayFilter): WorkItemNode[] {
  const matching = new Set<number>()

  for (const node of nodes) {
    if (matchesOverlay(node, filter)) {
      matching.add(node.id)
    }
  }

  if (matching.size === 0) {
    return []
  }

  const byId = new Map(nodes.map((node) => [node.id, node]))
  const visible = new Set(matching)

  for (const id of matching) {
    let current = byId.get(id)
    while (current?.parentId) {
      visible.add(current.parentId)
      current = byId.get(current.parentId)
    }
  }

  return nodes.filter((node) => visible.has(node.id))
}

function matchesOverlay(node: WorkItemNode, filter: OverlayFilter): boolean {
  if (filter.types && filter.types.length > 0 && !filter.types.includes(node.type)) {
    return false
  }
  if (filter.states && filter.states.length > 0 && !filter.states.includes(node.state)) {
    return false
  }
  if (filter.iterationPath && node.iterationPath !== filter.iterationPath) {
    return false
  }
  if (filter.assignee === 'unassigned' && node.assignedTo) {
    return false
  }
  if (filter.assignee === 'me') {
    const me = filter.currentUserUniqueName?.toLowerCase()
    if (!me || node.assignedTo?.uniqueName.toLowerCase() !== me) {
      return false
    }
  }
  return true
}
