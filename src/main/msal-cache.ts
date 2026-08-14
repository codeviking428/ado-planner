import type { ICachePlugin, TokenCacheContext } from '@azure/msal-node'

export type CacheStore = {
  persistToDisk: boolean
  read(): Promise<string | null>
  write(serialized: string): Promise<void>
  unlink(): Promise<void>
}

export function createMsalCachePlugin(store: CacheStore): ICachePlugin & {
  wipe(): Promise<void>
} {
  let memory = ''

  return {
    async beforeCacheAccess(context: TokenCacheContext): Promise<void> {
      if (store.persistToDisk) {
        const loaded = await store.read()
        if (loaded) {
          memory = loaded
        }
      }
      context.tokenCache.deserialize(memory)
    },
    async afterCacheAccess(context: TokenCacheContext): Promise<void> {
      if (!context.cacheHasChanged) {
        return
      }
      memory = context.tokenCache.serialize()
      if (store.persistToDisk) {
        await store.write(memory)
      }
    },
    async wipe(): Promise<void> {
      memory = ''
      await store.unlink()
    }
  }
}

export function shouldPersistSession(input: {
  platform: string
  encryptionAvailable: boolean
  storageBackend?: string
}): boolean {
  if (!input.encryptionAvailable) {
    return false
  }
  if (input.platform === 'linux') {
    const backend = input.storageBackend ?? 'unknown'
    return backend !== 'basic_text' && backend !== 'unknown'
  }
  return true
}
