export function adoAuthorizationHeader(token: string, scheme: 'bearer' | 'pat'): string {
  if (scheme === 'pat') {
    return `Basic ${Buffer.from(`:${token}`, 'utf8').toString('base64')}`
  }
  return `Bearer ${token}`
}
