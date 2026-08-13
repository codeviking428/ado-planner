import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import { START_DATE_FIELD, TARGET_DATE_FIELD } from '../src/shared/types'

export type WorkItemRecord = {
  id: number
  rev: number
  fields: Record<string, unknown>
}

const TYPES_WITH_DATES = new Set(['Epic', 'Feature', 'User Story', 'Task'])

function day(iso: string) {
  return `${iso}T00:00:00Z`
}

function seedItems(): WorkItemRecord[] {
  return [
    {
      id: 1001,
      rev: 12,
      fields: {
        'System.Title': 'Checkout rewrite',
        'System.WorkItemType': 'Epic',
        'System.State': 'New',
        'System.AreaPath': 'Shop\\Platform',
        'System.IterationPath': 'Shop\\FY26',
        [START_DATE_FIELD]: day('2026-07-01'),
        [TARGET_DATE_FIELD]: day('2026-09-30')
      }
    },
    {
      id: 1002,
      rev: 8,
      fields: {
        'System.Title': 'Cart',
        'System.WorkItemType': 'Feature',
        'System.State': 'Active',
        'System.Parent': 1001,
        'System.AreaPath': 'Shop\\Platform',
        'System.IterationPath': 'Shop\\FY26\\Sprint 11',
        [START_DATE_FIELD]: day('2026-07-06'),
        [TARGET_DATE_FIELD]: day('2026-08-14')
      }
    },
    {
      id: 1003,
      rev: 5,
      fields: {
        'System.Title': 'Persist cart',
        'System.WorkItemType': 'User Story',
        'System.State': 'Active',
        'System.Parent': 1002,
        'System.AssignedTo': { displayName: 'Ada Lovelace', uniqueName: 'ada@contoso.com' },
        'System.AreaPath': 'Shop\\Platform',
        'System.IterationPath': 'Shop\\FY26\\Sprint 10',
        'System.Description': '<p>Keep the cart</p>',
        [START_DATE_FIELD]: day('2026-07-06'),
        [TARGET_DATE_FIELD]: day('2026-07-20')
      }
    },
    {
      id: 1005,
      rev: 1,
      fields: {
        'System.Title': 'Cart UI',
        'System.WorkItemType': 'Task',
        'System.State': 'New',
        'System.Parent': 1003,
        'System.AreaPath': 'Shop\\Platform',
        'System.IterationPath': 'Shop\\FY26\\Sprint 12'
      }
    },
    {
      id: 1012,
      rev: 2,
      fields: {
        'System.Title': 'Unparented spike',
        'System.WorkItemType': 'Task',
        'System.State': 'Active',
        'System.AreaPath': 'Shop\\Platform',
        'System.IterationPath': 'Shop\\FY26\\Sprint 12'
      }
    }
  ]
}

function json(res: ServerResponse, status: number, body: unknown) {
  res.writeHead(status, { 'Content-Type': 'application/json' })
  res.end(JSON.stringify(body))
}

async function readBody(req: IncomingMessage): Promise<string> {
  const chunks: Buffer[] = []
  for await (const chunk of req) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
  }
  return Buffer.concat(chunks).toString('utf8')
}

const LAYOUT = {
  systemControls: [
    { id: 'System.Title', label: 'Title', visible: true, controlType: 'FieldControl' },
    { id: 'System.State', label: 'State', visible: true, controlType: 'FieldControl' }
  ],
  pages: [
    {
      id: 'details',
      label: 'Details',
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
                  id: 'System.AssignedTo',
                  label: 'Assigned To',
                  visible: true,
                  controlType: 'FieldControl'
                },
                {
                  id: 'System.Description',
                  label: 'Description',
                  visible: true,
                  controlType: 'HtmlFieldControl'
                }
              ]
            }
          ]
        }
      ]
    }
  ]
}

const TYPE_FIELDS = [
  { referenceName: 'System.Title', type: 'string', alwaysRequired: true },
  {
    referenceName: 'System.State',
    type: 'string',
    isPicklist: true,
    allowedValues: ['New', 'Active', 'Closed']
  },
  { referenceName: 'System.AssignedTo', type: 'string', isIdentity: true },
  { referenceName: 'System.Description', type: 'html' },
  { referenceName: START_DATE_FIELD, type: 'dateTime' },
  { referenceName: TARGET_DATE_FIELD, type: 'dateTime' }
]

export function startAdoMock(port = 0) {
  const items = seedItems()

  const server = createServer(async (req, res) => {
    const url = new URL(req.url ?? '/', 'http://127.0.0.1')
    const path = url.pathname
    const method = req.method ?? 'GET'

    try {
      if (path.endsWith('/_apis/profile/profiles/me')) {
        return json(res, 200, { id: 'user-1', displayName: 'Ada Lovelace' })
      }
      if (path.endsWith('/_apis/accounts')) {
        return json(res, 200, { value: [{ accountName: 'contoso' }] })
      }
      if (path.endsWith('/_apis/projects') && method === 'GET') {
        return json(res, 200, { value: [{ id: 'p1', name: 'Shop' }] })
      }
      if (path.includes('/_apis/projects/') && path.endsWith('/teams')) {
        return json(res, 200, { value: [{ id: 't1', name: 'Platform' }] })
      }
      if (path.includes('/teamfieldvalues')) {
        return json(res, 200, {
          values: [{ value: 'Shop\\Platform', includeChildren: true }]
        })
      }
      if (path.endsWith('/backlogs')) {
        return json(res, 200, {
          value: [
            { workItemTypes: [{ name: 'Epic' }] },
            { workItemTypes: [{ name: 'Feature' }] },
            { workItemTypes: [{ name: 'User Story' }] },
            { workItemTypes: [{ name: 'Task' }] }
          ]
        })
      }
      if (path.includes('/teamsettings/iterations')) {
        return json(res, 200, {
          value: [
            {
              name: 'Sprint 10',
              path: 'Shop\\FY26\\Sprint 10',
              attributes: { startDate: day('2026-07-13'), finishDate: day('2026-07-24') }
            },
            {
              name: 'Sprint 12',
              path: 'Shop\\FY26\\Sprint 12',
              attributes: { startDate: day('2026-08-10'), finishDate: day('2026-08-21') }
            }
          ]
        })
      }
      if (path.includes('/workitemtypes/') && path.includes('/fields')) {
        const type = decodeURIComponent(path.split('/workitemtypes/')[1]?.split('/')[0] ?? '')
        const fields = TYPES_WITH_DATES.has(type)
          ? TYPE_FIELDS
          : TYPE_FIELDS.filter(
              (field) =>
                field.referenceName !== START_DATE_FIELD &&
                field.referenceName !== TARGET_DATE_FIELD
            )
        return json(res, 200, { value: fields })
      }
      if (path.includes('/layout')) {
        return json(res, 200, LAYOUT)
      }
      if (path.includes('/wiql') && method === 'POST') {
        return json(res, 200, { workItems: items.map((item) => ({ id: item.id })) })
      }
      if (path.includes('/workitemsbatch') && method === 'POST') {
        const body = JSON.parse(await readBody(req)) as { ids?: number[] }
        const selected = items.filter((item) => body.ids?.includes(item.id))
        return json(res, 200, { value: selected })
      }
      const workItemMatch = path.match(/\/workitems\/(\d+)/)
      if (workItemMatch) {
        const id = Number(workItemMatch[1])
        const item = items.find((row) => row.id === id)
        if (!item) {
          return json(res, 404, { message: 'not found' })
        }
        if (method === 'PATCH') {
          const document = JSON.parse(await readBody(req)) as Array<{
            op: string
            path: string
            value?: unknown
          }>
          const revTest = document.find((op) => op.op === 'test' && op.path === '/rev')
          if (revTest && revTest.value !== item.rev) {
            return json(res, 412, { message: 'rev mismatch' })
          }
          for (const op of document) {
            if (op.op === 'add' && op.path.startsWith('/fields/')) {
              item.fields[op.path.slice('/fields/'.length)] = op.value
            }
          }
          item.rev += 1
        }
        return json(res, 200, item)
      }
      if (path.includes('/identities')) {
        return json(res, 200, {
          value: [{ displayName: 'Ada Lovelace', uniqueName: 'ada@contoso.com' }]
        })
      }
      json(res, 404, { message: `unmocked ${method} ${path}` })
    } catch (error) {
      json(res, 500, { message: error instanceof Error ? error.message : 'mock error' })
    }
  })

  return new Promise<{ url: string; close: () => Promise<void> }>((resolve) => {
    server.listen(port, '127.0.0.1', () => {
      const address = server.address()
      const bound = typeof address === 'object' && address ? address.port : port
      resolve({
        url: `http://127.0.0.1:${bound}`,
        close: () =>
          new Promise((done) => {
            server.close(() => done())
          })
      })
    })
  })
}
