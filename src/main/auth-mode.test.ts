import { describe, expect, test } from 'vitest'
import { entraClientIdFromEnv } from './auth-mode'

describe('Entra client ID configuration', () => {
  test('missing or blank env means Entra is not configured', () => {
    expect(entraClientIdFromEnv({})).toBeNull()
    expect(entraClientIdFromEnv({ ENTRA_CLIENT_ID: '' })).toBeNull()
    expect(entraClientIdFromEnv({ ENTRA_CLIENT_ID: '   ' })).toBeNull()
    expect(entraClientIdFromEnv({ MAIN_VITE_ENTRA_CLIENT_ID: '' })).toBeNull()
  })

  test('placeholder UUID is treated as unset', () => {
    expect(
      entraClientIdFromEnv({ ENTRA_CLIENT_ID: '00000000-0000-0000-0000-000000000000' })
    ).toBeNull()
  })

  test('a real public-client ID is configured', () => {
    expect(entraClientIdFromEnv({ ENTRA_CLIENT_ID: '11111111-2222-3333-4444-555555555555' })).toBe(
      '11111111-2222-3333-4444-555555555555'
    )
  })

  test('whitespace ENTRA_CLIENT_ID does not hide MAIN_VITE_ENTRA_CLIENT_ID', () => {
    expect(
      entraClientIdFromEnv({
        ENTRA_CLIENT_ID: '   ',
        MAIN_VITE_ENTRA_CLIENT_ID: '11111111-2222-3333-4444-555555555555'
      })
    ).toBe('11111111-2222-3333-4444-555555555555')
  })
})
