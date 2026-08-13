import { describe, expect, test } from 'vitest'
import {
  buildFormPatch,
  draftFromFormValues,
  flattenLayout,
  formFieldName,
  referenceNameFromFormField,
  stripMnemonic,
  workItemFormDefaults,
  workItemFormSchema
} from './form-layout'
import type { FormControl } from './types'

describe('flattenLayout', () => {
  test('keeps visible custom pages and skips history, links, attachments, contributions', () => {
    const { systemControls, pages } = flattenLayout(
      {
        systemControls: [
          { id: 'System.Title', label: 'Titl&e', visible: true, controlType: 'FieldControl' },
          {
            id: 'System.History',
            label: 'History',
            visible: true,
            controlType: 'WorkItemLogControl'
          }
        ],
        pages: [
          {
            id: 'details',
            label: 'Detai&ls',
            visible: true,
            pageType: 'custom',
            sections: [
              {
                groups: [
                  {
                    id: 'planning',
                    label: 'Planning',
                    visible: true,
                    controls: [
                      {
                        id: 'System.State',
                        label: 'Stat&e',
                        visible: true,
                        controlType: 'FieldControl'
                      },
                      {
                        id: 'System.AssignedTo',
                        label: 'Assigned To',
                        visible: true,
                        controlType: 'FieldControl'
                      },
                      {
                        id: 'hidden',
                        label: 'Hidden',
                        visible: false,
                        controlType: 'FieldControl'
                      }
                    ]
                  },
                  {
                    id: 'marketplace',
                    label: 'Extension',
                    isContribution: true,
                    visible: true,
                    controls: [{ id: 'ext.field', visible: true }]
                  }
                ]
              }
            ]
          },
          { id: 'history', label: 'History', visible: true, pageType: 'history' },
          { id: 'links', label: 'Links', visible: true, pageType: 'links' }
        ]
      },
      [
        {
          referenceName: 'System.Title',
          type: 'string',
          alwaysRequired: true
        },
        {
          referenceName: 'System.State',
          type: 'string',
          isPicklist: true,
          allowedValues: ['New', 'Active']
        },
        {
          referenceName: 'System.AssignedTo',
          type: 'string',
          isIdentity: true
        }
      ]
    )

    expect(systemControls.map((c) => c.referenceName)).toEqual(['System.Title'])
    expect(systemControls[0]?.label).toBe('Title')
    expect(pages).toHaveLength(1)
    expect(pages[0]?.label).toBe('Details')
    expect(pages[0]?.groups).toHaveLength(1)
    const controls = pages[0]?.groups[0]?.controls ?? []
    expect(controls.map((c) => c.kind)).toEqual(['picklist', 'identity'])
    expect(controls.some((c) => c.id === 'hidden')).toBe(false)
  })

  test('strips ADO mnemonics', () => {
    expect(stripMnemonic('Stat&e')).toBe('State')
  })

  test('save PATCH tests rev and only dirty editable fields', () => {
    const document = buildFormPatch({
      rev: 9,
      original: { 'System.Title': 'Old', 'System.State': 'New' },
      draft: { 'System.Title': 'New title', 'System.State': 'New' },
      editable: [
        {
          id: 'System.Title',
          referenceName: 'System.Title',
          label: 'Title',
          kind: 'string',
          required: true,
          readOnly: false,
          visible: true
        },
        {
          id: 'System.State',
          referenceName: 'System.State',
          label: 'State',
          kind: 'picklist',
          required: true,
          readOnly: false,
          visible: true
        }
      ]
    })
    expect(document).toEqual([
      { op: 'test', path: '/rev', value: 9 },
      { op: 'add', path: '/fields/System.Title', value: 'New title' }
    ])
  })
})

describe('TanStack Form field map', () => {
  const title: FormControl = {
    id: 'System.Title',
    referenceName: 'System.Title',
    label: 'Title',
    kind: 'string',
    required: true,
    readOnly: false,
    visible: true
  }
  const assigned: FormControl = {
    id: 'System.AssignedTo',
    referenceName: 'System.AssignedTo',
    label: 'Assigned To',
    kind: 'identity',
    required: false,
    readOnly: false,
    visible: true
  }

  test('encodes dotted ADO reference names so TanStack Form does not nest them', () => {
    expect(formFieldName('System.Title')).toBe('System::Title')
    expect(referenceNameFromFormField('System::Title')).toBe('System.Title')
    expect(formFieldName('Microsoft.VSTS.Scheduling.StartDate')).toBe(
      'Microsoft::VSTS::Scheduling::StartDate'
    )
  })

  test('default values and submitted draft round-trip ADO field names for PATCH', () => {
    const defaults = workItemFormDefaults(
      {
        'System.Title': 'Persist cart',
        'System.AssignedTo': { displayName: 'Ada', uniqueName: 'ada@contoso.com' }
      },
      [title, assigned]
    )
    expect(defaults).toEqual({
      'System::Title': 'Persist cart',
      'System::AssignedTo': { displayName: 'Ada', uniqueName: 'ada@contoso.com' }
    })
    const submitted = {
      ...defaults,
      'System::Title': 'Persist cart (saved)'
    }
    const draft = draftFromFormValues(submitted)
    expect(draft['System.Title']).toBe('Persist cart (saved)')
    expect(draft['System.AssignedTo']).toEqual({
      displayName: 'Ada',
      uniqueName: 'ada@contoso.com'
    })
    const document = buildFormPatch({
      rev: 5,
      original: {
        'System.Title': 'Persist cart',
        'System.AssignedTo': { displayName: 'Ada', uniqueName: 'ada@contoso.com' }
      },
      draft,
      editable: [title, assigned]
    })
    expect(document).toEqual([
      { op: 'test', path: '/rev', value: 5 },
      { op: 'add', path: '/fields/System.Title', value: 'Persist cart (saved)' }
    ])
  })

  test('zod schema requires always-required visible fields', () => {
    const schema = workItemFormSchema([title, assigned])
    expect(
      schema.safeParse({
        'System::Title': '',
        'System::AssignedTo': null
      }).success
    ).toBe(false)
    expect(
      schema.safeParse({
        'System::Title': 'Cart',
        'System::AssignedTo': null
      }).success
    ).toBe(true)
  })
})
