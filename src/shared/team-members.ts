import { ASSIGNEE_SPECIALS, type IdentityValue } from './types'

const RESERVED = new Set<string>(ASSIGNEE_SPECIALS)

export type TeamMemberIdentityRow = {
  identity?: {
    displayName?: string
    uniqueName?: string
    id?: string
    isContainer?: boolean
    isDeletedInOrigin?: boolean
  }
}

export function mapTeamMemberIdentities(rows: TeamMemberIdentityRow[]): IdentityValue[] {
  const seen = new Set<string>()
  const members: IdentityValue[] = []
  for (const row of rows) {
    const identity = row.identity
    if (!identity || identity.isContainer || identity.isDeletedInOrigin) {
      continue
    }
    const uniqueName = (identity.uniqueName ?? identity.displayName ?? '').trim()
    const displayName = (identity.displayName ?? identity.uniqueName ?? '').trim()
    if (!uniqueName || !displayName || RESERVED.has(uniqueName.toLowerCase())) {
      continue
    }
    const key = uniqueName.toLowerCase()
    if (seen.has(key)) {
      continue
    }
    seen.add(key)
    members.push({
      displayName,
      uniqueName,
      id: identity.id
    })
  }
  return members.sort((a, b) => a.displayName.localeCompare(b.displayName))
}
