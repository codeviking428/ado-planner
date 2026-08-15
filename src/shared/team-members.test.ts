import { describe, expect, test } from 'vitest'
import { mapTeamMemberIdentities } from './team-members'

describe('mapTeamMemberIdentities', () => {
  test('keeps people and skips containers, deleted, and reserved names', () => {
    expect(
      mapTeamMemberIdentities([
        {
          identity: {
            displayName: 'Grace Hopper',
            uniqueName: 'grace@contoso.com',
            id: 'g1'
          }
        },
        {
          identity: {
            displayName: 'Ada Lovelace',
            uniqueName: 'ada@contoso.com',
            id: 'a1'
          }
        },
        { identity: { displayName: 'Platform', uniqueName: 'vstfs://', isContainer: true } },
        {
          identity: {
            displayName: 'Gone',
            uniqueName: 'gone@contoso.com',
            isDeletedInOrigin: true
          }
        },
        { identity: { displayName: 'Me', uniqueName: 'me' } },
        { identity: { displayName: 'Ada Lovelace', uniqueName: 'ada@contoso.com' } }
      ])
    ).toEqual([
      { displayName: 'Ada Lovelace', uniqueName: 'ada@contoso.com', id: 'a1' },
      { displayName: 'Grace Hopper', uniqueName: 'grace@contoso.com', id: 'g1' }
    ])
  })
})
