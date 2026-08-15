import { ipcMain } from 'electron'
import { buildFormPatch } from '@shared/form-layout'
import { buildStartTargetPatch } from '@shared/dates'
import {
  loginCredsSchema,
  openFormSchema,
  patchDatesSchema,
  saveFormSchema,
  scopeSchema,
  searchIdentitiesSchema,
  teamMembersSchema
} from '@shared/ipc'
import type { FormControl } from '@shared/types'
import { AdoClient } from './ado-client'
import { RestAdoClient, useRestAdo } from './ado-rest'
import type { TokenProvider } from './session'
import type { UpdaterBridge } from './updater'

export function registerIpc(tokens: TokenProvider, updater: UpdaterBridge): void {
  const ado = useRestAdo() ? new RestAdoClient(tokens) : new AdoClient(tokens)

  ipcMain.handle('session:get', () => tokens.getSessionInfo())
  ipcMain.handle('session:login', async (_event, payload: unknown) => {
    const info = await tokens.login(loginCredsSchema.parse(payload ?? undefined))
    if (tokens.scheme !== 'pat' || !info.signedIn) {
      return info
    }

    const organization = await tokens.getOrganization()
    if (!organization) {
      await tokens.logout()
      throw new Error('No Azure DevOps organization is configured')
    }
    try {
      await ado.listProjects(organization)
      return info
    } catch (error) {
      await tokens.logout()
      const message = error instanceof Error ? error.message : 'Unknown error'
      throw new Error(`Could not connect to Azure DevOps organization: ${message}`)
    }
  })
  ipcMain.handle('session:logout', () => tokens.logout())

  ipcMain.handle('ado:orgs', () => ado.listOrganizations())
  ipcMain.handle('ado:projects', (_event, org: string) => ado.listProjects(org))
  ipcMain.handle('ado:teams', (_event, payload: { org: string; project: string }) =>
    ado.listTeams(payload.org, payload.project)
  )
  ipcMain.handle('ado:team-members', (_event, payload: unknown) => {
    const input = teamMembersSchema.parse(payload)
    return ado.listTeamMembers(input.org, input.project, input.team)
  })
  ipcMain.handle(
    'ado:iterations',
    (_event, payload: { org: string; project: string; team: string }) =>
      ado.listIterations(payload.org, payload.project, payload.team)
  )
  ipcMain.handle('ado:hierarchy', (_event, payload: unknown) =>
    ado.loadHierarchy(scopeSchema.parse(payload))
  )
  ipcMain.handle('ado:patch-dates', async (_event, payload: unknown) => {
    const input = patchDatesSchema.parse(payload)
    const document = buildStartTargetPatch({
      rev: input.rev,
      startDate: new Date(input.startDate),
      targetDate: new Date(input.targetDate)
    })
    return ado.patchDates({
      org: input.org,
      project: input.project,
      id: input.id,
      document
    })
  })
  ipcMain.handle('ado:form', (_event, payload: unknown) => {
    const input = openFormSchema.parse(payload)
    return ado.loadForm(input.org, input.project, input.id)
  })
  ipcMain.handle('ado:save-form', async (_event, payload: unknown) => {
    const input = saveFormSchema.parse(payload)
    const document = buildFormPatch({
      rev: input.rev,
      original: input.original,
      draft: input.draft,
      editable: input.editable as FormControl[]
    })
    return ado.saveForm({
      org: input.org,
      project: input.project,
      id: input.id,
      document
    })
  })
  ipcMain.handle('ado:identities', (_event, payload: unknown) => {
    const input = searchIdentitiesSchema.parse(payload)
    return ado.searchIdentities(input.org, input.query)
  })

  ipcMain.handle('updater:prompt', () => updater.prompt)
  ipcMain.handle('updater:apply', () => updater.apply())
  ipcMain.handle('updater:snooze', () => updater.snooze())
}
