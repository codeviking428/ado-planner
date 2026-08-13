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
  test('login with a PAT signs in and returns that token', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(true) })
    const info = await provider.login({ pat: 'pat-secret-value' })
    expect(info.signedIn).toBe(true)
    expect(info.authMode).toBe('pat')
    expect(await provider.getAccessToken()).toBe('pat-secret-value')
  })

  test('login without a PAT and none stored stays signed out', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(true) })
    const info = await provider.login()
    expect(info.signedIn).toBe(false)
    expect(info.authMode).toBe('pat')
  })

  test('stored PAT restores Session without asking again', async () => {
    const store = memoryStore(true, 'pat-secret-value')
    const provider = createPatTokenProvider({ store })
    const info = await provider.getSessionInfo()
    expect(info.signedIn).toBe(true)
    expect(await provider.getAccessToken()).toBe('pat-secret-value')
  })

  test('logout wipes the stored PAT', async () => {
    const store = memoryStore(true)
    const provider = createPatTokenProvider({ store })
    await provider.login({ pat: 'pat-secret-value' })
    await provider.logout()
    expect(store.unlinked).toBe(true)
    expect((await provider.getSessionInfo()).signedIn).toBe(false)
  })

  test('memory-only store never writes the PAT', async () => {
    const store = memoryStore(false)
    const provider = createPatTokenProvider({ store })
    await provider.login({ pat: 'pat-secret-value' })
    expect(store.writes).toEqual([])
    expect(await provider.getAccessToken()).toBe('pat-secret-value')
  })

  test('blank PAT is rejected', async () => {
    const provider = createPatTokenProvider({ store: memoryStore(true) })
    await expect(provider.login({ pat: '   ' })).rejects.toThrow(/PAT/)
  })
})
