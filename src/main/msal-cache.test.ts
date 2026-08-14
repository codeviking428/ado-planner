import { describe, expect, test } from 'vitest'
import { createMsalCachePlugin, shouldPersistSession } from './msal-cache'

describe('Session cache policy', () => {
  test('Windows persists when encryption is available without a storage backend', () => {
    expect(shouldPersistSession({ platform: 'win32', encryptionAvailable: true })).toBe(true)
  })

  test('macOS persists when encryption is available without a storage backend', () => {
    expect(shouldPersistSession({ platform: 'darwin', encryptionAvailable: true })).toBe(true)
  })

  test('Windows is memory-only when encryption is unavailable', () => {
    expect(shouldPersistSession({ platform: 'win32', encryptionAvailable: false })).toBe(false)
  })

  test('Linux gnome_libsecret persists when encryption is available', () => {
    expect(
      shouldPersistSession({
        platform: 'linux',
        encryptionAvailable: true,
        storageBackend: 'gnome_libsecret'
      })
    ).toBe(true)
  })

  test('Linux basic_text is memory-only', () => {
    expect(
      shouldPersistSession({
        platform: 'linux',
        encryptionAvailable: true,
        storageBackend: 'basic_text'
      })
    ).toBe(false)
  })

  test('Linux unknown backend is memory-only', () => {
    expect(
      shouldPersistSession({
        platform: 'linux',
        encryptionAvailable: true,
        storageBackend: 'unknown'
      })
    ).toBe(false)
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
