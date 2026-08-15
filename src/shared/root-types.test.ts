import { describe, expect, test } from 'vitest'
import { resolveRootTypes, topBacklogTypesFromLevels } from './root-types'

describe('topBacklogTypesFromLevels', () => {
  test('picks the highest rank when ranks are present', () => {
    expect(
      topBacklogTypesFromLevels([
        { rank: 10, workItemTypes: [{ name: 'Task' }] },
        { rank: 30, workItemTypes: [{ name: 'Epic' }, { name: 'Portfolio Epic' }] },
        { rank: 20, workItemTypes: [{ name: 'Feature' }] }
      ])
    ).toEqual(['Epic', 'Portfolio Epic'])
  })

  test('uses the first level when ranks are missing', () => {
    expect(
      topBacklogTypesFromLevels([
        { workItemTypes: [{ name: 'Epic' }] },
        { workItemTypes: [{ name: 'Feature' }] }
      ])
    ).toEqual(['Epic'])
  })
})

describe('resolveRootTypes', () => {
  const loaded = ['Epic', 'Feature', 'Task']

  test('missing prefs seeds the top backlog types that exist', () => {
    expect(
      resolveRootTypes({
        stored: undefined,
        loadedTypes: loaded,
        topBacklogTypes: ['Epic']
      })
    ).toEqual(['Epic'])
  })

  test('missing prefs and no top backlog means every loaded type is a Root type', () => {
    expect(
      resolveRootTypes({
        stored: undefined,
        loadedTypes: loaded,
        topBacklogTypes: []
      })
    ).toBeNull()
  })

  test('empty stored list is Hide all', () => {
    expect(
      resolveRootTypes({ stored: [], loadedTypes: loaded, topBacklogTypes: ['Epic'] })
    ).toEqual([])
  })

  test('null stored list is Show all', () => {
    expect(
      resolveRootTypes({ stored: null, loadedTypes: loaded, topBacklogTypes: ['Epic'] })
    ).toBeNull()
  })

  test('intersects stored names with loaded types', () => {
    expect(
      resolveRootTypes({
        stored: ['Epic', 'Technical Roadmap Item'],
        loadedTypes: loaded,
        topBacklogTypes: ['Epic']
      })
    ).toEqual(['Epic'])
  })

  test('empty intersection reseeds from the top backlog', () => {
    expect(
      resolveRootTypes({
        stored: ['Technical Roadmap Item'],
        loadedTypes: loaded,
        topBacklogTypes: ['Epic']
      })
    ).toEqual(['Epic'])
  })
})
