import * as azdev from 'azure-devops-node-api'
import type { IWorkItemTrackingApi } from 'azure-devops-node-api/WorkItemTrackingApi'
import type { IWorkApi } from 'azure-devops-node-api/WorkApi'
import type { ICoreApi } from 'azure-devops-node-api/CoreApi'
import type { IWorkItemTrackingProcessApi } from 'azure-devops-node-api/WorkItemTrackingProcessApi'
import {
  JsonPatchDocument,
  JsonPatchOperation,
  Operation
} from 'azure-devops-node-api/interfaces/common/VSSInterfaces'
import {
  BATCH_SIZE,
  START_DATE_FIELD,
  TARGET_DATE_FIELD,
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

function adoOrgUrl(org: string): string {
  const base = process.env.ADO_PLANNER_ADO_BASE_URL
  if (base) {
    return `${base.replace(/\/$/, '')}/${org}`
  }
  return `https://dev.azure.com/${org}`
}

function vsspsUrl(): string {
  return (process.env.ADO_PLANNER_VSSPS_URL ?? 'https://app.vssps.visualstudio.com').replace(
    /\/$/,
    ''
  )
}

async function restJson(
  url: string,
  token: string,
  scheme: 'bearer' | 'pat',
  init?: RequestInit
): Promise<unknown> {
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
    return null
  }
  return response.json()
}

export class AdoClient {
  constructor(private readonly tokens: TokenProvider) {}

  private async connect(org: string): Promise<azdev.WebApi> {
    const token = await this.tokens.getAccessToken()
    const handler =
      this.tokens.scheme === 'pat'
        ? azdev.getPersonalAccessTokenHandler(token)
        : azdev.getBearerHandler(token)
    return new azdev.WebApi(adoOrgUrl(org), handler)
  }

  private async wit(org: string): Promise<IWorkItemTrackingApi> {
    return (await this.connect(org)).getWorkItemTrackingApi()
  }

  private async work(org: string): Promise<IWorkApi> {
    return (await this.connect(org)).getWorkApi()
  }

  private async core(org: string): Promise<ICoreApi> {
    return (await this.connect(org)).getCoreApi()
  }

  private async processApi(org: string): Promise<IWorkItemTrackingProcessApi> {
    return (await this.connect(org)).getWorkItemTrackingProcessApi()
  }

  async listOrganizations(): Promise<Organization[]> {
    const token = await this.tokens.getAccessToken()
    const scheme = this.tokens.scheme
    const profile = (await restJson(
      `${vsspsUrl()}/_apis/profile/profiles/me?api-version=7.1`,
      token,
      scheme
    )) as { id?: string }
    const memberQuery = profile.id ? `memberId=${profile.id}&` : ''
    const payload = (await restJson(
      `${vsspsUrl()}/_apis/accounts?${memberQuery}api-version=7.1`,
      token,
      scheme
    )) as
      | { value?: Array<{ accountName?: string }>; accountName?: string }
      | Array<{ accountName?: string }>
    const values = Array.isArray(payload) ? payload : (payload.value ?? [])
    return values
      .map((row) => row.accountName)
      .filter((name): name is string => Boolean(name))
      .map((accountName) => ({ accountName }))
  }

  async listProjects(org: string): Promise<Project[]> {
    const core = await this.core(org)
    const page = await core.getProjects()
    return (page ?? []).map((project) => ({
      id: project.id ?? project.name ?? '',
      name: project.name ?? ''
    }))
  }

  async listTeams(org: string, project: string): Promise<Team[]> {
    const core = await this.core(org)
    const teams = await core.getTeams(project)
    return (teams ?? []).map((team) => ({
      id: team.id ?? team.name ?? '',
      name: team.name ?? ''
    }))
  }

  async listIterations(org: string, project: string, team: string): Promise<IterationNode[]> {
    const work = await this.work(org)
    const iterations = await work.getTeamIterations({ project, team })
    return (iterations ?? []).map((iteration) => ({
      path: iteration.path ?? iteration.name ?? '',
      name: iteration.name ?? '',
      startDate: iteration.attributes?.startDate
        ? new Date(iteration.attributes.startDate).toISOString()
        : null,
      finishDate: iteration.attributes?.finishDate
        ? new Date(iteration.attributes.finishDate).toISOString()
        : null,
      children: []
    }))
  }

  async loadHierarchy(scope: ScopeSelection): Promise<HierarchyResult> {
    const work = await this.work(scope.org)
    const wit = await this.wit(scope.org)
    const teamContext = { project: scope.project, team: scope.team }

    const [fieldValues, backlogs] = await Promise.all([
      work.getTeamFieldValues(teamContext),
      work.getBacklogs(teamContext)
    ])

    const types = [
      ...new Set(
        (backlogs ?? []).flatMap((level) =>
          (level.workItemTypes ?? []).map((type) => type.name).filter(Boolean)
        )
      )
    ] as string[]

    const areas = (fieldValues.values ?? []).map((value) => ({
      value: value.value ?? '',
      includeChildren: value.includeChildren !== false
    }))

    const wiql = {
      query: buildHierarchyWiql({ project: scope.project, types, areas })
    }
    const result = await wit.queryByWiql(wiql, { project: scope.project })
    const ids = (result.workItems ?? [])
      .map((item) => item.id)
      .filter((id): id is number => id != null)
    const truncated = ids.length >= WIQL_RESULT_CAP

    const typeDateFields = new Set<string>()
    await Promise.all(
      types.map(async (type) => {
        const fields = await wit.getWorkItemTypeFieldsWithReferences(scope.project, type)
        const names = (fields ?? []).map((field) => field.referenceName ?? '').filter(Boolean)
        if (typeHasStartAndTarget(names)) {
          typeDateFields.add(type)
        }
      })
    )

    const nodes: import('@shared/types').WorkItemNode[] = []
    for (const chunk of chunkIds(ids, BATCH_SIZE)) {
      if (chunk.length === 0) {
        continue
      }
      const batch = await wit.getWorkItemsBatch({
        ids: chunk,
        fields: HIERARCHY_FIELDS,
        errorPolicy: 2
      })
      for (const item of batch ?? []) {
        if (item.id) {
          nodes.push(
            mapBatchWorkItem({ id: item.id, rev: item.rev, fields: item.fields }, typeDateFields)
          )
        }
      }
    }

    return {
      nodes: assembleForest(nodes),
      types,
      truncated
    }
  }

  async patchDates(input: {
    org: string
    project: string
    id: number
    document: JsonPatchOp[]
  }): Promise<{ rev: number }> {
    const wit = await this.wit(input.org)
    const patch = toVssPatch(input.document)
    const updated = await wit.updateWorkItem(undefined, patch, input.id, input.project, false)
    return {
      rev: updated.rev ?? (input.document.find((op) => op.path === '/rev')?.value as number)
    }
  }

  async loadForm(org: string, project: string, id: number): Promise<WorkItemFormModel> {
    const wit = await this.wit(org)
    const core = await this.core(org)
    const processApi = await this.processApi(org)
    const item = await wit.getWorkItem(id, undefined, undefined, undefined, project)
    const typeName = String(item.fields?.['System.WorkItemType'] ?? '')
    const projects = await core.getProjects()
    const projectRow = (projects ?? []).find((row) => row.name === project || row.id === project)
    const props = projectRow?.id
      ? await core.getProjectProperties(projectRow.id, ['System.CurrentProcessTemplateId'])
      : []
    const processId = props.find((prop) => prop.name === 'System.CurrentProcessTemplateId')?.value
    let layout: ProcessLayout = { systemControls: [], pages: [] }
    let witRefName = typeName
    if (processId) {
      const processTypes = await processApi.getProcessWorkItemTypes(String(processId))
      const match = (processTypes ?? []).find((row) => row.name === typeName)
      witRefName = match?.referenceName ?? typeName
      layout = (await processApi.getFormLayout(String(processId), witRefName)) as ProcessLayout
    }
    const typeFields = await wit.getWorkItemTypeFieldsWithReferences(project, typeName)
    const catalog = await wit.getFields(project)
    const metaByRef = new Map<string, FieldMetadata>()
    for (const field of catalog ?? []) {
      if (field.referenceName) {
        metaByRef.set(field.referenceName, {
          referenceName: field.referenceName,
          name: field.name,
          type: String(field.type ?? 'string'),
          isIdentity: field.isIdentity,
          isPicklist: field.isPicklist,
          readOnly: field.readOnly
        })
      }
    }
    for (const field of typeFields ?? []) {
      if (!field.referenceName) {
        continue
      }
      const existing = metaByRef.get(field.referenceName) ?? {
        referenceName: field.referenceName
      }
      metaByRef.set(field.referenceName, {
        ...existing,
        alwaysRequired: field.alwaysRequired,
        allowedValues: field.allowedValues as string[] | undefined
      })
    }
    const flattened = flattenLayout(layout, [...metaByRef.values()])
    return {
      id,
      rev: item.rev ?? 1,
      type: typeName,
      systemControls: flattened.systemControls,
      pages: flattened.pages,
      values: (item.fields ?? {}) as Record<string, unknown>
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
    const token = await this.tokens.getAccessToken()
    const scheme = this.tokens.scheme
    const url = `${adoOrgUrl(org).replace(`/${org}`, '')}/_apis/identities?searchFilter=General&filterValue=${encodeURIComponent(query)}&api-version=7.1`
    const vssps = `${process.env.ADO_PLANNER_ADO_BASE_URL ?? `https://vssps.dev.azure.com/${org}`}`
    const payload = (await restJson(
      `${vssps.replace(/\/$/, '')}/_apis/identities?searchFilter=General&filterValue=${encodeURIComponent(query)}&api-version=7.1`,
      token,
      scheme
    ).catch(() => restJson(url, token, scheme))) as {
      value?: Array<{ displayName?: string; uniqueName?: string; id?: string }>
    }
    return (payload.value ?? [])
      .filter((row) => row.displayName || row.uniqueName)
      .map((row) => ({
        displayName: row.displayName ?? row.uniqueName ?? '',
        uniqueName: row.uniqueName ?? row.displayName ?? '',
        id: row.id
      }))
  }
}

function toVssPatch(document: JsonPatchOp[]): JsonPatchDocument {
  return document.map((op) => {
    const operation: JsonPatchOperation = {
      op:
        op.op === 'test'
          ? Operation.Test
          : op.op === 'remove'
            ? Operation.Remove
            : op.op === 'replace'
              ? Operation.Replace
              : Operation.Add,
      path: op.path,
      value: op.value
    }
    return operation
  })
}

export { START_DATE_FIELD, TARGET_DATE_FIELD }
