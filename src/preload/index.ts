import { contextBridge, ipcRenderer } from 'electron'
import type {
  HierarchyResult,
  IdentityValue,
  IterationNode,
  Organization,
  Project,
  ScopeSelection,
  SessionInfo,
  Team,
  UpdaterPrompt,
  WorkItemFormModel
} from '@shared/types'
import type { OverlayFilter } from '@shared/types'

export type PlannerApi = {
  session: {
    get(): Promise<SessionInfo>
    login(): Promise<SessionInfo>
    logout(): Promise<void>
  }
  ado: {
    orgs(): Promise<Organization[]>
    projects(org: string): Promise<Project[]>
    teams(org: string, project: string): Promise<Team[]>
    iterations(org: string, project: string, team: string): Promise<IterationNode[]>
    hierarchy(scope: ScopeSelection): Promise<HierarchyResult>
    patchDates(input: {
      org: string
      project: string
      id: number
      rev: number
      startDate: string
      targetDate: string
    }): Promise<{ rev: number }>
    form(org: string, project: string, id: number): Promise<WorkItemFormModel>
    saveForm(input: {
      org: string
      project: string
      id: number
      rev: number
      original: Record<string, unknown>
      draft: Record<string, unknown>
      editable: unknown[]
    }): Promise<{ rev: number }>
    identities(org: string, query: string): Promise<IdentityValue[]>
  }
  updater: {
    prompt(): Promise<UpdaterPrompt | null>
    apply(): Promise<void>
    snooze(): Promise<void>
    onAvailable(handler: (prompt: UpdaterPrompt) => void): () => void
    onError(handler: (message: string) => void): () => void
  }
}

const api: PlannerApi = {
  session: {
    get: () => ipcRenderer.invoke('session:get'),
    login: () => ipcRenderer.invoke('session:login'),
    logout: () => ipcRenderer.invoke('session:logout')
  },
  ado: {
    orgs: () => ipcRenderer.invoke('ado:orgs'),
    projects: (org) => ipcRenderer.invoke('ado:projects', org),
    teams: (org, project) => ipcRenderer.invoke('ado:teams', { org, project }),
    iterations: (org, project, team) =>
      ipcRenderer.invoke('ado:iterations', { org, project, team }),
    hierarchy: (scope) => ipcRenderer.invoke('ado:hierarchy', scope),
    patchDates: (input) => ipcRenderer.invoke('ado:patch-dates', input),
    form: (org, project, id) => ipcRenderer.invoke('ado:form', { org, project, id }),
    saveForm: (input) => ipcRenderer.invoke('ado:save-form', input),
    identities: (org, query) => ipcRenderer.invoke('ado:identities', { org, query })
  },
  updater: {
    prompt: () => ipcRenderer.invoke('updater:prompt'),
    apply: () => ipcRenderer.invoke('updater:apply'),
    snooze: () => ipcRenderer.invoke('updater:snooze'),
    onAvailable(handler) {
      const listener = (_event: unknown, prompt: UpdaterPrompt) => handler(prompt)
      ipcRenderer.on('updater:available', listener)
      return () => ipcRenderer.removeListener('updater:available', listener)
    },
    onError(handler) {
      const listener = (_event: unknown, message: string) => handler(message)
      ipcRenderer.on('updater:error', listener)
      return () => ipcRenderer.removeListener('updater:error', listener)
    }
  }
}

contextBridge.exposeInMainWorld('planner', api)

export type { OverlayFilter }
