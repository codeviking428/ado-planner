import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import { START_DATE_FIELD, TARGET_DATE_FIELD } from '../src/shared/types'

export type WorkItemRecord = {
  id: number
  rev: number
  fields: Record<string, unknown>
}

type PendingResponse = {
  path: string
  delayMs: number
  failure?: { status: number; message: string }
}

export type AdoMock = {
  url: string
  delayNext: (path: string, delayMs: number) => void
  failNext: (path: string, status: number, message: string) => void
  close: () => Promise<void>
}

const FIXTURES = join(__dirname, 'fixtures')

function loadJson<T>(name: string): T {
  return JSON.parse(readFileSync(join(FIXTURES, name), 'utf8')) as T
}

const TYPES_WITH_DATES = new Set(
  loadJson<{ typesWithDates: string[] }>('type-fields.json').typesWithDates
)

function seedItems(): WorkItemRecord[] {
  return structuredClone(loadJson<WorkItemRecord[]>('work-items.json'))
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

const LAYOUT = loadJson('layout.json')
const TYPE_FIELDS_FILE = loadJson<{
  withDates: Array<Record<string, unknown>>
}>('type-fields.json')
const CONNECTION_DATA = loadJson('connection-data.json')
const PROFILE = loadJson('profile.json')
const ACCOUNTS = loadJson('accounts.json')
const PROJECTS = loadJson<Record<string, { value: unknown }>>('projects.json')
const TEAMS = loadJson<Record<string, { value: unknown }>>('teams.json')
const MEMBERS = loadJson<Record<string, { value: unknown }>>('members.json')
const ITERATIONS = loadJson('iterations.json')
const TEAM_FIELD_VALUES = loadJson('team-field-values.json')
const BACKLOGS = loadJson('backlogs.json')
const IDENTITIES = loadJson('identities.json')
const PROCESS = loadJson<{
  processId: string
  project: unknown
  workItemTypes: unknown
}>('process.json')

export function startAdoMock(port = 0) {
  const items = seedItems()
  const pendingResponses: PendingResponse[] = []

  const server = createServer(async (req, res) => {
    const url = new URL(req.url ?? '/', 'http://127.0.0.1')
    const path = url.pathname
    const method = req.method ?? 'GET'

    try {
      const pendingIndex = pendingResponses.findIndex((pending) => path.includes(pending.path))
      if (pendingIndex >= 0) {
        const [pending] = pendingResponses.splice(pendingIndex, 1)
        if (pending.delayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, pending.delayMs))
        }
        if (pending.failure) {
          return json(res, pending.failure.status, { message: pending.failure.message })
        }
      }

      if (path.endsWith('/_apis/connectionData')) {
        return json(res, 200, CONNECTION_DATA)
      }
      if (path.endsWith('/_apis/profile/profiles/me')) {
        return json(res, 200, PROFILE)
      }
      if (path.endsWith('/_apis/accounts')) {
        return json(res, 200, ACCOUNTS)
      }
      if (path.endsWith('/_apis/projects') && method === 'GET') {
        return json(
          res,
          200,
          path.startsWith('/fabrikam/') ? PROJECTS.fabrikam : PROJECTS.contoso
        )
      }
      if (path.includes('/_apis/projects/') && method === 'GET' && !path.includes('/teams')) {
        const projectMatch = path.match(/\/_apis\/projects\/([^/]+)$/)
        if (projectMatch) {
          return json(res, 200, PROCESS.project)
        }
      }
      if (path.includes('/teams/') && path.endsWith('/members')) {
        return json(
          res,
          200,
          path.includes('/Delivery/') || path.includes('/Roadmap/')
            ? MEMBERS.Delivery
            : MEMBERS.Platform
        )
      }
      if (path.includes('/_apis/projects/') && path.endsWith('/teams')) {
        return json(res, 200, path.includes('/Roadmap/') ? TEAMS.Roadmap : TEAMS.Shop)
      }
      if (path.includes('/teamfieldvalues')) {
        return json(res, 200, TEAM_FIELD_VALUES)
      }
      if (path.endsWith('/backlogs')) {
        return json(res, 200, BACKLOGS)
      }
      if (path.includes('/teamsettings/iterations')) {
        return json(res, 200, ITERATIONS)
      }
      if (path.includes('/work/processes/') && path.endsWith('/workItemTypes')) {
        return json(res, 200, PROCESS.workItemTypes)
      }
      if (path.includes('/workitemtypes/') && path.includes('/fields')) {
        const type = decodeURIComponent(path.split('/workitemtypes/')[1]?.split('/')[0] ?? '')
        const fields = TYPES_WITH_DATES.has(type)
          ? TYPE_FIELDS_FILE.withDates
          : TYPE_FIELDS_FILE.withDates.filter(
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
        return json(res, 200, IDENTITIES)
      }
      json(res, 404, { message: `unmocked ${method} ${path}` })
    } catch (error) {
      json(res, 500, { message: error instanceof Error ? error.message : 'mock error' })
    }
  })

  return new Promise<AdoMock>((resolve) => {
    server.listen(port, '127.0.0.1', () => {
      const address = server.address()
      const bound = typeof address === 'object' && address ? address.port : port
      resolve({
        url: `http://127.0.0.1:${bound}`,
        delayNext(path, delayMs) {
          pendingResponses.push({ path, delayMs })
        },
        failNext(path, status, message) {
          pendingResponses.push({ path, delayMs: 0, failure: { status, message } })
        },
        close: () =>
          new Promise((done) => {
            server.close(() => done())
          })
      })
    })
  })
}
