import { describe, expect, test } from 'vitest'
import type { CacheStore } from './msal-cache'
import { createPatTokenProvider } from './pat-session'

function memoryStore(
  persistToDisk: boolean,
  initial: string | null = null
): CacheStore & {
  writes: string[]
  unlinked: boolean
} {
  let value = initial
  const writes: string[] = []
  return {
    persistToDisk,
    writes,
    unlinked: false,
    async read() {
      return value
    },
    async write(serialized) {
      writes.push(serialized)
      value = serialized
    },
    async unlink() {
      this.unlinked = true
      value = null
    }
  }
}

function createSession(
  store = memoryStore(true),
  resolveIdentity: () => Promise<{ displayName: string; username: string } | null> = async () =>
    null
) {
  return createPatTokenProvider({ store, resolveIdentity })
}

describe('PAT Session', () => {
  test('login with a PAT and organization signs in and returns both', async () => {
    const tokens = createSession()
    const info = await tokens.login({
      pat: 'pat-secret-value',
      organization: 'https://dev.azure.com/contoso/'
    })
    expect(info.signedIn).toBe(true)
    expect(info.authMode).toBe('pat')
    expect(await tokens.getAccessToken()).toBe('pat-secret-value')
    expect(await tokens.getOrganization()).toBe('contoso')
  })

  test('login without a PAT is rejected', async () => {
    const tokens = createSession()
    await expect(tokens.login()).rejects.toThrow(/PAT/)
  })

  test('stored credentials restore Session without asking again', async () => {
    const store = memoryStore(
      true,
      JSON.stringify({ pat: 'pat-secret-value', organization: 'contoso' })
    )
    const tokens = createSession(store)
    const info = await tokens.getSessionInfo()
    expect(info.signedIn).toBe(true)
    expect(await tokens.getAccessToken()).toBe('pat-secret-value')
    expect(await tokens.getOrganization()).toBe('contoso')
  })

  test('logout wipes the stored PAT', async () => {
    const store = memoryStore(true)
    const tokens = createSession(store)
    await tokens.login({ pat: 'pat-secret-value', organization: 'contoso' })
    await tokens.logout()
    expect(store.unlinked).toBe(true)
    expect((await tokens.getSessionInfo()).signedIn).toBe(false)
  })

  test('memory-only store never writes the PAT', async () => {
    const store = memoryStore(false)
    const tokens = createSession(store)
    await tokens.login({ pat: 'pat-secret-value', organization: 'contoso' })
    expect(store.writes).toEqual([])
    expect(await tokens.getAccessToken()).toBe('pat-secret-value')
  })

  test('blank PAT is rejected', async () => {
    const tokens = createSession()
    await expect(tokens.login({ pat: '   ', organization: 'contoso' })).rejects.toThrow(/PAT/)
  })

  test('missing organization is rejected', async () => {
    const tokens = createSession()
    await expect(tokens.login({ pat: 'pat-secret-value' })).rejects.toThrow(/organization URL/)
  })

  test('organization URLs with extra path are shortened to the org slug', async () => {
    const tokens = createSession(memoryStore(false))
    await tokens.login({
      pat: 'pat-secret-value',
      organization: 'https://dev.azure.com/contoso/Shop/_workitems/edit/123'
    })
    expect(await tokens.getOrganization()).toBe('contoso')
  })

  test('legacy visualstudio.com organization URLs are normalized', async () => {
    const tokens = createSession(memoryStore(false))
    await tokens.login({
      pat: 'pat-secret-value',
      organization: 'https://contoso.visualstudio.com/'
    })
    expect(await tokens.getOrganization()).toBe('contoso')
  })

  test('legacy PAT-only storage does not create an incomplete Session', async () => {
    const tokens = createSession(memoryStore(true, 'pat-secret-value'))
    expect((await tokens.getSessionInfo()).signedIn).toBe(false)
  })

  test('resolved identity fills displayName and username', async () => {
    const tokens = createSession(memoryStore(false), async () => ({
      displayName: 'Ada Lovelace',
      username: 'ada@contoso.com'
    }))
    const info = await tokens.login({ pat: 'pat-secret-value', organization: 'contoso' })
    expect(info.displayName).toBe('Ada Lovelace')
    expect(info.username).toBe('ada@contoso.com')
  })
})
