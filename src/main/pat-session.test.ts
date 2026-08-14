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

describe('PAT Session', () => {
  test('login with a PAT and organization signs in and returns both', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(true) })
    const info = await provider.login({
      pat: 'pat-secret-value',
      organization: 'https://dev.azure.com/contoso/'
    })
    expect(info.signedIn).toBe(true)
    expect(info.authMode).toBe('pat')
    expect(await provider.getAccessToken()).toBe('pat-secret-value')
    expect(await provider.getOrganization()).toBe('contoso')
  })

  test('login without a PAT is rejected', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(true) })
    await expect(provider.login()).rejects.toThrow(/PAT/)
  })

  test('stored credentials restore Session without asking again', async () => {
    const store = memoryStore(
      true,
      JSON.stringify({ pat: 'pat-secret-value', organization: 'contoso' })
    )
    const provider = createPatTokenProvider({ store })
    const info = await provider.getSessionInfo()
    expect(info.signedIn).toBe(true)
    expect(await provider.getAccessToken()).toBe('pat-secret-value')
    expect(await provider.getOrganization()).toBe('contoso')
  })

  test('logout wipes the stored PAT', async () => {
    const store = memoryStore(true)
    const provider = createPatTokenProvider({ store })
    await provider.login({ pat: 'pat-secret-value', organization: 'contoso' })
    await provider.logout()
    expect(store.unlinked).toBe(true)
    expect((await provider.getSessionInfo()).signedIn).toBe(false)
  })

  test('memory-only store never writes the PAT', async () => {
    const store = memoryStore(false)
    const provider = createPatTokenProvider({ store })
    await provider.login({ pat: 'pat-secret-value', organization: 'contoso' })
    expect(store.writes).toEqual([])
    expect(await provider.getAccessToken()).toBe('pat-secret-value')
  })

  test('blank PAT is rejected', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(true) })
    await expect(provider.login({ pat: '   ', organization: 'contoso' })).rejects.toThrow(/PAT/)
  })

  test('missing organization is rejected', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(true) })
    await expect(provider.login({ pat: 'pat-secret-value' })).rejects.toThrow(/organization URL/)
  })

  test('organization URLs with extra path are shortened to the org slug', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(false) })
    await provider.login({
      pat: 'pat-secret-value',
      organization: 'https://dev.azure.com/contoso/Shop/_workitems/edit/123'
    })
    expect(await provider.getOrganization()).toBe('contoso')
  })

  test('legacy visualstudio.com organization URLs are normalized', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(false) })
    await provider.login({
      pat: 'pat-secret-value',
      organization: 'https://contoso.visualstudio.com/'
    })
    expect(await provider.getOrganization()).toBe('contoso')
  })

  test('legacy PAT-only storage does not create an incomplete Session', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(true, 'pat-secret-value') })
    expect((await provider.getSessionInfo()).signedIn).toBe(false)
  })
})
