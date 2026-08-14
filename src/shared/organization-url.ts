const ORG_SLUG = /^[a-z0-9][a-z0-9-]{0,49}$/i
const LEGACY_HOST = /^([a-z0-9][a-z0-9-]{0,49})\.visualstudio\.com$/i

function parseUrl(value: string): URL | null {
  try {
    return new URL(value)
  } catch {
    try {
      return new URL(`https://${value}`)
    } catch {
      return null
    }
  }
}

/** Keep `https://dev.azure.com/{org}` (or the visualstudio.com host) and drop path, query, and hash. */
export function shortenOrganizationUrl(value: string): string {
  const input = value.trim()
  if (!input) {
    return value
  }

  const url = parseUrl(input)
  if (!url) {
    return value
  }

  const hostname = url.hostname.toLowerCase()
  if (hostname === 'dev.azure.com' || hostname === 'www.dev.azure.com') {
    const org = url.pathname.split('/').filter(Boolean)[0]
    if (!org || !ORG_SLUG.test(org)) {
      return value
    }
    return `https://dev.azure.com/${org}`
  }

  const legacy = LEGACY_HOST.exec(url.hostname)
  if (legacy) {
    return `https://${legacy[1]}.visualstudio.com`
  }

  return value
}
