import { shortenOrganizationUrl } from '@shared/organization-url'
import type { SessionInfo } from '@shared/types'
import type { CacheStore } from './msal-cache'
import { fetchPatIdentity, type PatIdentity } from './pat-identity'
import type { TokenProvider } from './session'

type PatCredentials = {
  pat: string
  organization: string
}

function normalizeOrganization(value: string): string {
  const input = shortenOrganizationUrl(value).trim()
  if (/^[a-z0-9][a-z0-9-]{0,49}$/i.test(input)) {
    return input
  }

  let url: URL
  try {
    url = new URL(input)
  } catch {
    throw new Error('Enter an Azure DevOps organization URL such as https://dev.azure.com/contoso')
  }

  if (url.protocol !== 'https:' || url.search || url.hash) {
    throw new Error('Enter an Azure DevOps organization URL such as https://dev.azure.com/contoso')
  }

  const segments = url.pathname.split('/').filter(Boolean)
  if (url.hostname.toLowerCase() === 'dev.azure.com' && segments.length === 1) {
    return normalizeOrganization(segments[0])
  }

  const legacy = /^([a-z0-9][a-z0-9-]{0,49})\.visualstudio\.com$/i.exec(url.hostname)
  if (legacy && segments.length === 0) {
    return legacy[1]
  }

  throw new Error('Enter an Azure DevOps organization URL such as https://dev.azure.com/contoso')
}

function deserializeCredentials(serialized: string | null): PatCredentials | null {
  if (!serialized) {
    return null
  }
  try {
    const value = JSON.parse(serialized) as Partial<PatCredentials>
    if (typeof value.pat !== 'string' || typeof value.organization !== 'string') {
      return null
    }
    const pat = value.pat.trim()
    if (!pat) {
      return null
    }
    return { pat, organization: normalizeOrganization(value.organization) }
  } catch {
    return null
  }
}

export function createPatTokenProvider(input: {
  store: CacheStore
  resolveIdentity?: (org: string, pat: string) => Promise<PatIdentity | null>
}): TokenProvider {
  let memory: PatCredentials | null = null
  let loaded = false
  let identity: PatIdentity | null | undefined
  const resolveIdentity = input.resolveIdentity ?? fetchPatIdentity

  const load = async (): Promise<PatCredentials | null> => {
    if (!loaded) {
      loaded = true
      if (input.store.persistToDisk) {
        memory = deserializeCredentials(await input.store.read())
      }
    }
    return memory
  }

  const info = async (): Promise<SessionInfo> => {
    const pat = await load()
    if (!pat) {
      return { signedIn: false, displayName: null, username: null, authMode: 'pat' }
    }
    if (identity === undefined) {
      try {
        identity = await resolveIdentity(pat.organization, pat.pat)
      } catch {
        identity = null
      }
    }
    return {
      signedIn: true,
      displayName: identity?.displayName ?? 'PAT',
      username: identity?.username ?? null,
      authMode: 'pat'
    }
  }

  return {
    scheme: 'pat',
    async getAccessToken() {
      const credentials = await load()
      if (!credentials) {
        throw new Error('No PAT Session')
      }
      return credentials.pat
    },
    async getOrganization() {
      return (await load())?.organization ?? null
    },
    getSessionInfo: info,
    async login(creds) {
      const pat = creds?.pat?.trim() ?? ''
      if (!pat) {
        throw new Error('PAT is required')
      }
      const organization = normalizeOrganization(creds?.organization ?? '')
      memory = { pat, organization }
      identity = undefined
      loaded = true
      if (input.store.persistToDisk) {
        await input.store.write(JSON.stringify(memory))
      }
      return info()
    },
    async logout() {
      memory = null
      identity = undefined
      loaded = true
      await input.store.unlink()
    }
  }
}
