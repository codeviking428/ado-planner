import {
  BATCH_SIZE,
  WIQL_RESULT_CAP,
  type HierarchyResult,
  type IdentityValue,
  type IterationNode,
  type JsonPatchOp,
  type Organization,
  type Project,
  type ScopeSelection,
  type Team,
  type WorkItemFormModel
} from '@shared/types'
import {
  assembleForest,
  buildHierarchyWiql,
  chunkIds,
  HIERARCHY_FIELDS,
  mapBatchWorkItem
} from '@shared/hierarchy'
import { typeHasStartAndTarget } from '@shared/dates'
import { flattenLayout, type FieldMetadata, type ProcessLayout } from '@shared/form-layout'
import { adoAuthorizationHeader } from './ado-auth'
import type { TokenProvider } from './session'

function baseUrl(): string {
  return (process.env.ADO_PLANNER_ADO_BASE_URL ?? '').replace(/\/$/, '')
}

function vsspsUrl(): string {
  return (process.env.ADO_PLANNER_VSSPS_URL ?? baseUrl()).replace(/\/$/, '')
}

async function rest<T>(
  token: string,
  scheme: 'bearer' | 'pat',
  url: string,
  init?: RequestInit
): Promise<T> {
  const response = await fetch(url, {
    ...init,
    headers: {
      Authorization: adoAuthorizationHeader(token, scheme),
      Accept: 'application/json',
      'Content-Type': 'application/json',
      ...(init?.headers ?? {})
    }
  })
  if (!response.ok) {
    const body = await response.text()
    throw new Error(`${response.status} ${url}: ${body.slice(0, 400)}`)
  }
  if (response.status === 204) {
    return undefined as T
  }
  return (await response.json()) as T
}

type Collection<T> = { value?: T[] }

export class RestAdoClient {
  constructor(private readonly tokens: TokenProvider) {}

  private async token(): Promise<string> {
    return this.tokens.getAccessToken()
  }

  private async request<T>(url: string, init?: RequestInit): Promise<T> {
    return rest<T>(await this.token(), this.tokens.scheme, url, init)
  }

  private orgUrl(org: string): string {
    return `${baseUrl()}/${org}`
  }

  async listOrganizations(): Promise<Organization[]> {
    if (this.tokens.scheme === 'pat') {
      const organization = await this.tokens.getOrganization()
      if (!organization) {
        throw new Error('No Azure DevOps organization is configured')
      }
      return [{ accountName: organization }]
    }

    const profile = await this.request<{ id?: string }>(
      `${vsspsUrl()}/_apis/profile/profiles/me?api-version=7.1`
    )
    const qs = profile.id ? `memberId=${profile.id}&` : ''
    const payload = await this.request<Collection<{ accountName?: string }>>(
      `${vsspsUrl()}/_apis/accounts?${qs}api-version=7.1`
    )
    return (payload.value ?? [])
      .map((row) => row.accountName)
      .filter((name): name is string => Boolean(name))
      .map((accountName) => ({ accountName }))
  }

  async listProjects(org: string): Promise<Project[]> {
    const payload = await this.request<Collection<{ id?: string; name?: string }>>(
      `${this.orgUrl(org)}/_apis/projects?api-version=7.1`
    )
    return (payload.value ?? []).map((row) => ({
      id: row.id ?? row.name ?? '',
      name: row.name ?? ''
    }))
  }

  async listTeams(org: string, project: string): Promise<Team[]> {
    const payload = await this.request<Collection<{ id?: string; name?: string }>>(
      `${this.orgUrl(org)}/_apis/projects/${encodeURIComponent(project)}/teams?api-version=7.1`
    )
    return (payload.value ?? []).map((row) => ({
      id: row.id ?? row.name ?? '',
      name: row.name ?? ''
    }))
  }

  async listIterations(org: string, project: string, team: string): Promise<IterationNode[]> {
    const payload = await this.request<
      Collection<{
        path?: string
        name?: string
        attributes?: { startDate?: string; finishDate?: string }
      }>
    >(
      `${this.orgUrl(org)}/${encodeURIComponent(project)}/${encodeURIComponent(team)}/_apis/work/teamsettings/iterations?api-version=7.1`
    )
    return (payload.value ?? []).map((row) => ({
      path: row.path ?? row.name ?? '',
      name: row.name ?? '',
      startDate: row.attributes?.startDate ?? null,
      finishDate: row.attributes?.finishDate ?? null,
      children: []
    }))
  }

  async loadHierarchy(scope: ScopeSelection): Promise<HierarchyResult> {
    const teamRoot = `${this.orgUrl(scope.org)}/${encodeURIComponent(scope.project)}/${encodeURIComponent(scope.team)}`
    const projectRoot = `${this.orgUrl(scope.org)}/${encodeURIComponent(scope.project)}`
    const [fieldValues, backlogs] = await Promise.all([
      this.request<{ values?: Array<{ value?: string; includeChildren?: boolean }> }>(
        `${teamRoot}/_apis/work/teamsettings/teamfieldvalues?api-version=7.1`
      ),
      this.request<Collection<{ workItemTypes?: Array<{ name?: string }> }>>(
        `${teamRoot}/_apis/work/backlogs?api-version=7.1`
      )
    ])
    const types = [
      ...new Set(
        (backlogs.value ?? []).flatMap((level) =>
          (level.workItemTypes ?? []).map((type) => type.name).filter(Boolean)
        )
      )
    ] as string[]
    const areas = (fieldValues.values ?? []).map((value) => ({
      value: value.value ?? '',
      includeChildren: value.includeChildren !== false
    }))
    const wiql = await this.request<{ workItems?: Array<{ id?: number }> }>(
      `${projectRoot}/_apis/wit/wiql?api-version=7.1`,
      {
        method: 'POST',
        body: JSON.stringify({
          query: buildHierarchyWiql({ project: scope.project, types, areas })
        })
      }
    )
    const ids = (wiql.workItems ?? [])
      .map((item) => item.id)
      .filter((id): id is number => id != null)
    const typeDateFields = new Set<string>()
    for (const type of types) {
      const fields = await this.request<Collection<{ referenceName?: string }>>(
        `${projectRoot}/_apis/wit/workitemtypes/${encodeURIComponent(type)}/fields?api-version=7.1`
      )
      const names = (fields.value ?? []).map((field) => field.referenceName ?? '')
      if (typeHasStartAndTarget(names)) {
        typeDateFields.add(type)
      }
    }
    const nodes: import('@shared/types').WorkItemNode[] = []
    for (const chunk of chunkIds(ids, BATCH_SIZE)) {
      const batch = await this.request<
        Collection<{ id?: number; rev?: number; fields?: Record<string, unknown> }>
      >(`${projectRoot}/_apis/wit/workitemsbatch?api-version=7.1`, {
        method: 'POST',
        body: JSON.stringify({ ids: chunk, fields: HIERARCHY_FIELDS, errorPolicy: 'Omit' })
      })
      for (const item of batch.value ?? []) {
        if (item.id) {
          nodes.push(
            mapBatchWorkItem({ id: item.id, rev: item.rev, fields: item.fields }, typeDateFields)
          )
        }
      }
    }
    return { nodes: assembleForest(nodes), types, truncated: ids.length >= WIQL_RESULT_CAP }
  }

  async patchDates(input: {
    org: string
    project: string
    id: number
    document: JsonPatchOp[]
  }): Promise<{ rev: number }> {
    const updated = await this.request<{ rev?: number }>(
      `${this.orgUrl(input.org)}/${encodeURIComponent(input.project)}/_apis/wit/workitems/${input.id}?api-version=7.1`,
      { method: 'PATCH', body: JSON.stringify(input.document) }
    )
    return { rev: updated.rev ?? 0 }
  }

  async loadForm(org: string, project: string, id: number): Promise<WorkItemFormModel> {
    const projectRoot = `${this.orgUrl(org)}/${encodeURIComponent(project)}`
    const item = await this.request<{
      id?: number
      rev?: number
      fields?: Record<string, unknown>
    }>(`${projectRoot}/_apis/wit/workitems/${id}?api-version=7.1`)
    const typeName = String(item.fields?.['System.WorkItemType'] ?? '')
    const layout = await this.request<ProcessLayout>(
      `${this.orgUrl(org)}/_apis/work/processes/agile/workItemTypes/${encodeURIComponent(typeName)}/layout?api-version=7.1`
    ).catch(() => ({ systemControls: [], pages: [] }))
    const fields = await this.request<Collection<FieldMetadata>>(
      `${projectRoot}/_apis/wit/workitemtypes/${encodeURIComponent(typeName)}/fields?$expand=All&api-version=7.1`
    )
    const flattened = flattenLayout(layout, fields.value ?? [])
    return {
      id,
      rev: item.rev ?? 1,
      type: typeName,
      systemControls: flattened.systemControls,
      pages: flattened.pages,
      values: item.fields ?? {}
    }
  }

  async saveForm(input: {
    org: string
    project: string
    id: number
    document: JsonPatchOp[]
  }): Promise<{ rev: number }> {
    return this.patchDates(input)
  }

  async searchIdentities(org: string, query: string): Promise<IdentityValue[]> {
    const payload = await this.request<Collection<IdentityValue>>(
      `${this.orgUrl(org)}/_apis/identities?searchFilter=General&filterValue=${encodeURIComponent(query)}&api-version=7.1`
    )
    return payload.value ?? []
  }
}

export function useRestAdo(): boolean {
  return Boolean(process.env.ADO_PLANNER_ADO_BASE_URL)
}
