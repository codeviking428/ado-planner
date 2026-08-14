import { describe, expect, test } from 'vitest'
import { shortenOrganizationUrl } from './organization-url'

describe('shortenOrganizationUrl', () => {
  test('keeps only the org slug on a pasted Azure DevOps URL', () => {
    expect(
      shortenOrganizationUrl(
        'https://dev.azure.com/contoso/Shop/_workitems/edit/123?_a=edit#discussion'
      )
    ).toBe('https://dev.azure.com/contoso')
  })

  test('strips a trailing slash from an org URL', () => {
    expect(shortenOrganizationUrl('https://dev.azure.com/contoso/')).toBe(
      'https://dev.azure.com/contoso'
    )
  })

  test('leaves an already-short org URL unchanged', () => {
    expect(shortenOrganizationUrl('https://dev.azure.com/contoso')).toBe(
      'https://dev.azure.com/contoso'
    )
  })

  test('accepts a URL without a scheme', () => {
    expect(shortenOrganizationUrl('dev.azure.com/contoso/Shop/_boards')).toBe(
      'https://dev.azure.com/contoso'
    )
  })

  test('shortens a legacy visualstudio.com URL to the org host', () => {
    expect(shortenOrganizationUrl('https://contoso.visualstudio.com/Shop/_workitems/edit/1')).toBe(
      'https://contoso.visualstudio.com'
    )
  })

  test('leaves an org slug alone', () => {
    expect(shortenOrganizationUrl('contoso')).toBe('contoso')
  })

  test('leaves incomplete or unrelated input alone', () => {
    expect(shortenOrganizationUrl('https://dev.azure.com/')).toBe('https://dev.azure.com/')
    expect(shortenOrganizationUrl('https://github.com/contoso/repo')).toBe(
      'https://github.com/contoso/repo'
    )
    expect(shortenOrganizationUrl('not a url')).toBe('not a url')
  })
})
