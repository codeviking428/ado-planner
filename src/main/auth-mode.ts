const PLACEHOLDER_CLIENT_ID = '00000000-0000-0000-0000-000000000000'

export function entraClientIdFromEnv(
  env: Record<string, string | undefined> = process.env
): string | null {
  const raw = (env.ENTRA_CLIENT_ID ?? '').trim() || (env.MAIN_VITE_ENTRA_CLIENT_ID ?? '').trim()
  if (!raw || raw === PLACEHOLDER_CLIENT_ID) {
    return null
  }
  return raw
}
