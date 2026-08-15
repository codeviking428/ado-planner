import { describe, expect, test } from 'vitest'
import { identityFromConnectionData, identityFromProfile } from './pat-identity'

describe('PAT identity', () => {
  test('prefers uniqueName and display name from connectionData', () => {
    expect(
      identityFromConnectionData({
        authenticatedUser: {
          uniqueName: 'ada@contoso.com',
          providerDisplayName: 'Ada Lovelace',
          customDisplayName: 'Ada'
        }
      })
    ).toEqual({ displayName: 'Ada', username: 'ada@contoso.com' })
  })

  test('falls back to Mail then Account properties', () => {
    expect(
      identityFromConnectionData({
        authenticatedUser: {
          providerDisplayName: 'Ada Lovelace',
          properties: { Mail: { $value: 'ada@contoso.com' } }
        }
      })
    ).toEqual({ displayName: 'Ada Lovelace', username: 'ada@contoso.com' })
    expect(
      identityFromConnectionData({
        authenticatedUser: {
          providerDisplayName: 'Ada Lovelace',
          properties: { Account: { $value: 'CONTOSO\\ada' } }
        }
      })
    ).toEqual({ displayName: 'Ada Lovelace', username: 'CONTOSO\\ada' })
  })

  test('profile uses emailAddress as the uniqueName', () => {
    expect(
      identityFromProfile({ displayName: 'Ada Lovelace', emailAddress: 'ada@contoso.com' })
    ).toEqual({ displayName: 'Ada Lovelace', username: 'ada@contoso.com' })
  })

  test('empty payloads are ignored', () => {
    expect(identityFromConnectionData({})).toBeNull()
    expect(identityFromProfile({ displayName: 'Ada Lovelace' })).toBeNull()
  })
})
