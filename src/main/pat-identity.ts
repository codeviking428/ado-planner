import { adoAuthorizationHeader } from './ado-auth'

export type PatIdentity = {
  displayName: string
  username: string
}

type IdentityProperties = Record<string, { $value?: string } | string | undefined>

type ConnectionDataPayload = {
  authenticatedUser?: {
    uniqueName?: string
    providerDisplayName?: string
    customDisplayName?: string
    properties?: IdentityProperties
  }
}

type ProfilePayload = {
  displayName?: string
  emailAddress?: string
}

function propertyValue(properties: IdentityProperties | undefined, key: string): string {
  const raw = properties?.[key]
  if (typeof raw === 'string') {
    return raw.trim()
  }
  if (raw && typeof raw === 'object' && typeof raw.$value === 'string') {
    return raw.$value.trim()
  }
  return ''
}

export function identityFromConnectionData(payload: ConnectionDataPayload): PatIdentity | null {
  const user = payload.authenticatedUser
  if (!user) {
    return null
  }
  const username =
    user.uniqueName?.trim() ||
    propertyValue(user.properties, 'Mail') ||
    propertyValue(user.properties, 'Account')
  const displayName = user.customDisplayName?.trim() || user.providerDisplayName?.trim() || username
  if (!username) {
    return displayName ? { displayName, username: displayName } : null
  }
  return { displayName: displayName || username, username }
}

export function identityFromProfile(payload: ProfilePayload): PatIdentity | null {
  const username = payload.emailAddress?.trim() || ''
  const displayName = payload.displayName?.trim() || username
  if (!username) {
    return null
  }
  return { displayName: displayName || username, username }
}

function adoBaseUrl(): string {
  return (process.env.ADO_PLANNER_ADO_BASE_URL ?? 'https://dev.azure.com').replace(/\/$/, '')
}

function vsspsBaseUrl(org: string): string {
  if (process.env.ADO_PLANNER_VSSPS_URL) {
    return process.env.ADO_PLANNER_VSSPS_URL.replace(/\/$/, '')
  }
  if (process.env.ADO_PLANNER_ADO_BASE_URL) {
    return adoBaseUrl()
  }
  return `https://vssps.dev.azure.com/${org}`
}

async function getJson<T>(url: string, pat: string): Promise<T | null> {
  const response = await fetch(url, {
    headers: {
      Authorization: adoAuthorizationHeader(pat, 'pat'),
      Accept: 'application/json'
    }
  })
  if (!response.ok) {
    return null
  }
  return (await response.json()) as T
}

export async function fetchPatIdentity(org: string, pat: string): Promise<PatIdentity | null> {
  const [connection, profile] = await Promise.all([
    getJson<ConnectionDataPayload>(
      `${adoBaseUrl()}/${org}/_apis/connectionData?api-version=7.1`,
      pat
    ),
    getJson<ProfilePayload>(`${vsspsBaseUrl(org)}/_apis/profile/profiles/me?api-version=7.1`, pat)
  ])
  return (
    (profile ? identityFromProfile(profile) : null) ??
    (connection ? identityFromConnectionData(connection) : null)
  )
}
