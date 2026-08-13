import { describe, expect, test } from 'vitest'
import { adoAuthorizationHeader } from './ado-auth'

describe('ADO Authorization header', () => {
  test('PAT uses HTTP Basic with an empty username', () => {
    expect(adoAuthorizationHeader('pat-secret-value', 'pat')).toBe('Basic OnBhdC1zZWNyZXQtdmFsdWU=')
  })

  test('Entra Session uses Bearer', () => {
    expect(adoAuthorizationHeader('eyJhbGciOiJIUzI1NiJ9', 'bearer')).toBe(
      'Bearer eyJhbGciOiJIUzI1NiJ9'
    )
  })
})
