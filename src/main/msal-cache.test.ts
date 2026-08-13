import { describe, expect, test } from 'vitest'
import { createMsalCachePlugin, shouldPersistSession } from './msal-cache'

describe('Session cache policy', () => {
  test('Linux basic_text is memory-only', () => {
    expect(shouldPersistSession({ platform: 'linux', storageBackend: 'basic_text' })).toBe(false)
  })

  test('Windows DPAPI persists to disk', () => {
    expect(shouldPersistSession({ platform: 'win32', storageBackend: 'dpapi' })).toBe(true)
  })

  test('plugin writes only when persistToDisk and cache changed', async () => {
    const writes: string[] = []
    const plugin = createMsalCachePlugin({
      persistToDisk: true,
      read: async () => '{"Account":{}}',
      write: async (value) => {
        writes.push(value)
      },
      unlink: async () => undefined
    })
    const cache = {
      deserialize: (value: string) => {
        expect(value).toBe('{"Account":{}}')
      },
      serialize: () => '{"Account":{"x":1}}'
    }
    await plugin.beforeCacheAccess({
      cacheHasChanged: false,
      tokenCache: cache
    } as never)
    await plugin.afterCacheAccess({
      cacheHasChanged: true,
      tokenCache: cache
    } as never)
    expect(writes).toEqual(['{"Account":{"x":1}}'])
  })

  test('memory-only plugin never writes', async () => {
    let wrote = false
    const plugin = createMsalCachePlugin({
      persistToDisk: false,
      read: async () => {
        throw new Error('should not read')
      },
      write: async () => {
        wrote = true
      },
      unlink: async () => undefined
    })
    await plugin.afterCacheAccess({
      cacheHasChanged: true,
      tokenCache: { serialize: () => 'secret' }
    } as never)
    expect(wrote).toBe(false)
  })
})
