export const ADO_RESOURCE_SCOPE = '499b84ac-1321-427f-aa17-267ca6975798/.default'
export const ENTRA_AUTHORITY = 'https://login.microsoftonline.com/organizations'
export const START_DATE_FIELD = 'Microsoft.VSTS.Scheduling.StartDate'
export const TARGET_DATE_FIELD = 'Microsoft.VSTS.Scheduling.TargetDate'
export const WIQL_RESULT_CAP = 20_000
export const BATCH_SIZE = 200

export type IdentityValue = {
  displayName: string
  uniqueName: string
  id?: string
}

export type WorkItemNode = {
  id: number
  rev: number
  title: string
  type: string
  state: string
  assignedTo: IdentityValue | null
  parentId: number | null
  areaPath: string
  iterationPath: string
  startDate: string | null
  targetDate: string | null
  hasDateFields: boolean
  fields: Record<string, unknown>
}

export type HierarchyResult = {
  nodes: WorkItemNode[]
  types: string[]
  truncated: boolean
}

export type AssigneeFilter = 'anyone' | 'me' | 'unassigned'

export type OverlayFilter = {
  types: string[] | null
  states: string[] | null
  assignee: AssigneeFilter
  iterationPath: string | null
  currentUserUniqueName?: string | null
}

export type ScopeSelection = {
  org: string
  project: string
  team: string
  iterationPath?: string | null
}

export type JsonPatchOp = {
  op: 'test' | 'add' | 'remove' | 'replace'
  path: string
  value?: unknown
}

export type FormControlKind =
  | 'string'
  | 'html'
  | 'identity'
  | 'picklist'
  | 'integer'
  | 'double'
  | 'boolean'
  | 'dateTime'
  | 'plainText'
  | 'treePath'

export type FormControl = {
  id: string
  referenceName: string
  label: string
  kind: FormControlKind
  required: boolean
  readOnly: boolean
  visible: boolean
  options?: string[]
}

export type FormGroup = {
  id: string
  label: string
  controls: FormControl[]
}

export type FormPage = {
  id: string
  label: string
  groups: FormGroup[]
}

export type WorkItemFormModel = {
  id: number
  rev: number
  type: string
  systemControls: FormControl[]
  pages: FormPage[]
  values: Record<string, unknown>
}

export type AuthMode = 'entra' | 'pat'

export type SessionInfo = {
  signedIn: boolean
  displayName: string | null
  username: string | null
  authMode: AuthMode
}

export type Organization = { accountName: string }
export type Project = { id: string; name: string }
export type Team = { id: string; name: string }
export type IterationNode = {
  path: string
  name: string
  startDate: string | null
  finishDate: string | null
  children: IterationNode[]
}

export type UpdaterPrompt = {
  version: string
  releaseNotes: string | null
}
