import type { SessionInfo } from '@shared/types'
import type { CacheStore } from './msal-cache'
import type { TokenProvider } from './session'

export function createPatTokenProvider(input: { store: CacheStore }): TokenProvider {
  let memory: string | null = null
  let loaded = false

  const load = async (): Promise<string | null> => {
    if (!loaded) {
      loaded = true
      if (input.store.persistToDisk) {
        memory = await input.store.read()
      }
    }
    return memory
  }

  const info = async (): Promise<SessionInfo> => {
    const pat = await load()
    if (!pat) {
      return { signedIn: false, displayName: null, username: null, authMode: 'pat' }
    }
    return { signedIn: true, displayName: 'PAT', username: null, authMode: 'pat' }
  }

  return {
    scheme: 'pat',
    async getAccessToken() {
      const pat = await load()
      if (!pat) {
        throw new Error('No PAT Session')
      }
      return pat
    },
    getSessionInfo: info,
    async login(creds) {
      if (creds?.pat !== undefined) {
        const pat = creds.pat.trim()
        if (!pat) {
          throw new Error('PAT is required')
        }
        memory = pat
        loaded = true
        if (input.store.persistToDisk) {
          await input.store.write(pat)
        }
      }
      return info()
    },
    async logout() {
      memory = null
      loaded = true
      await input.store.unlink()
    }
  }
}
