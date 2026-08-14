import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { PaletteIcon } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Spinner } from '@/components/ui/spinner'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from '@/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger
} from '@/components/ui/dropdown-menu'
import { useFlavor } from '@/components/flavor-provider'
import { FilterMenu } from '@/components/filter-menu'
import { HierarchyGantt } from '@/components/hierarchy-gantt'
import { ScopeField } from '@/components/scope-field'
import { WorkItemFormDialog } from '@/components/work-item-form'
import { showErrorToast } from '@/lib/error-toast'
import { applyOverlays } from '@shared/overlays'
import { FLAVORS, type Flavor } from '@shared/flavor'
import { shortenOrganizationUrl } from '@shared/organization-url'
import type {
  AssigneeFilter,
  OverlayFilter,
  ScopeSelection,
  SessionInfo,
  UpdaterPrompt,
  WorkItemFormModel,
  WorkItemNode
} from '@shared/types'

const FLAVOR_LABELS: Record<Flavor, string> = {
  latte: 'Latte',
  frappe: 'Frappé',
  macchiato: 'Macchiato',
  mocha: 'Mocha'
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) {
    return '?'
  }
  if (parts.length === 1) {
    return parts[0].slice(0, 2).toUpperCase()
  }
  return `${parts[0][0]}${parts[parts.length - 1][0]}`.toUpperCase()
}

function useQueryErrorToast(error: unknown, fallback: string): void {
  useEffect(() => {
    if (error) {
      showErrorToast(error, fallback)
    }
  }, [error, fallback])
}

function SignInShell({
  session,
  onLogin
}: {
  session: SessionInfo | undefined
  onLogin: (creds?: { pat?: string; organization?: string }) => void
}) {
  const [pat, setPat] = useState('')
  const [organization, setOrganization] = useState('')
  const patMode = session?.authMode === 'pat'
  return (
    <div
      className="bg-background flex h-svh items-center justify-center p-6"
      data-testid="sign-in-shell"
    >
      <div className="bg-card w-full max-w-md rounded-2xl border p-8 shadow-sm">
        <p className="text-muted-foreground text-[11px] font-medium tracking-wider uppercase">
          Azure DevOps
        </p>
        <h1 className="mt-1 text-2xl font-semibold tracking-tight">ADO Planner</h1>
        {patMode ? (
          <div className="mt-6 flex flex-col gap-4">
            <p className="text-muted-foreground text-sm leading-relaxed">
              Entra app ID is not set. Paste an organization URL and a personal access token to
              continue while SSO is unavailable. Scopes: Work (read & write) and Project.
            </p>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="organization-input">Organization</Label>
              <Input
                id="organization-input"
                type="url"
                autoCapitalize="none"
                autoComplete="url"
                spellCheck={false}
                data-testid="organization-input"
                placeholder="https://dev.azure.com/your-organization"
                value={organization}
                onChange={(event) => setOrganization(shortenOrganizationUrl(event.target.value))}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="pat-input">Personal access token</Label>
              <Input
                id="pat-input"
                type="password"
                autoComplete="off"
                data-testid="pat-input"
                placeholder="Personal access token"
                value={pat}
                onChange={(event) => setPat(event.target.value)}
              />
            </div>
            <Button
              className="mt-1"
              onClick={() => onLogin({ pat, organization })}
              disabled={!pat.trim() || !organization.trim()}
              data-testid="sign-in"
            >
              Continue
            </Button>
          </div>
        ) : (
          <div className="mt-6 flex flex-col gap-4">
            <p className="text-muted-foreground text-sm leading-relaxed">
              Sign in with your work or school account to view a Team Work Item Hierarchy on a
              Gantt.
            </p>
            <Button onClick={() => onLogin()} data-testid="sign-in">
              Sign in
            </Button>
          </div>
        )}
        {session && !session.signedIn ? (
          <p className="text-muted-foreground mt-4 text-xs">No Session yet.</p>
        ) : null}
      </div>
    </div>
  )
}

function PlannerApp() {
  const { flavor, setFlavor } = useFlavor()
  const [org, setOrg] = useState('')
  const [project, setProject] = useState('')
  const [team, setTeam] = useState('')
  const [iterationPath, setIterationPath] = useState('')
  const [assignee, setAssignee] = useState<AssigneeFilter>('anyone')
  const [hiddenTypes, setHiddenTypes] = useState<string[]>([])
  const [hiddenStates, setHiddenStates] = useState<string[]>([])
  const [nodes, setNodes] = useState<WorkItemNode[]>([])
  const [openId, setOpenId] = useState<number | null>(null)
  const [form, setForm] = useState<WorkItemFormModel | null>(null)
  const [updater, setUpdater] = useState<UpdaterPrompt | null>(null)

  const sessionQuery = useQuery({
    queryKey: ['session'],
    queryFn: () => window.planner.session.get()
  })

  const login = useMutation({
    mutationFn: (creds?: { pat?: string; organization?: string }) =>
      window.planner.session.login(creds),
    onSuccess: (info) => {
      sessionQuery.refetch()
      if (!info.signedIn) {
        showErrorToast('Sign-in did not create a Session')
      }
    },
    onError: (error: Error) => showErrorToast(error, 'Sign-in failed')
  })

  const orgsQuery = useQuery({
    queryKey: ['orgs'],
    queryFn: () => window.planner.ado.orgs(),
    enabled: sessionQuery.data?.signedIn === true
  })

  const projectsQuery = useQuery({
    queryKey: ['projects', org],
    queryFn: () => window.planner.ado.projects(org),
    enabled: Boolean(org)
  })

  const teamsQuery = useQuery({
    queryKey: ['teams', org, project],
    queryFn: () => window.planner.ado.teams(org, project),
    enabled: Boolean(org && project)
  })

  const iterationsQuery = useQuery({
    queryKey: ['iterations', org, project, team],
    queryFn: () => window.planner.ado.iterations(org, project, team),
    enabled: Boolean(org && project && team)
  })

  const scope: ScopeSelection | null =
    org && project && team ? { org, project, team, iterationPath: iterationPath || null } : null

  const hierarchyQuery = useQuery({
    queryKey: ['hierarchy', org, project, team],
    queryFn: () => window.planner.ado.hierarchy(scope as ScopeSelection),
    enabled: Boolean(scope)
  })
  const scopeIsFetching =
    orgsQuery.isFetching ||
    projectsQuery.isFetching ||
    teamsQuery.isFetching ||
    iterationsQuery.isFetching ||
    hierarchyQuery.isFetching

  useQueryErrorToast(sessionQuery.error, 'Could not start Session')
  useQueryErrorToast(orgsQuery.error, 'Could not load organizations')
  useQueryErrorToast(projectsQuery.error, 'Could not load projects')
  useQueryErrorToast(teamsQuery.error, 'Could not load Teams')
  useQueryErrorToast(iterationsQuery.error, 'Could not load iterations')
  useQueryErrorToast(hierarchyQuery.error, 'Could not load Hierarchy')

  useEffect(() => {
    if (hierarchyQuery.data) {
      setNodes(hierarchyQuery.data.nodes)
      if (hierarchyQuery.data.truncated) {
        toast.warning('Hierarchy is incomplete — WIQL hit the 20,000 result cap.')
      }
    }
  }, [hierarchyQuery.data])

  useEffect(() => {
    const offAvailable = window.planner.updater.onAvailable(setUpdater)
    const offError = window.planner.updater.onError((message) =>
      showErrorToast(message, 'Updater failed')
    )
    void window.planner.updater
      .prompt()
      .then((prompt) => {
        if (prompt) {
          setUpdater(prompt)
        }
      })
      .catch((error: unknown) => showErrorToast(error, 'Could not check for updates'))
    return () => {
      offAvailable()
      offError()
    }
  }, [])

  useEffect(() => {
    if (!org && orgsQuery.data?.[0]) {
      setOrg(orgsQuery.data[0].accountName)
    }
  }, [org, orgsQuery.data])

  useEffect(() => {
    if (!project && projectsQuery.data?.[0]) {
      setProject(projectsQuery.data[0].name)
    }
  }, [project, projectsQuery.data])

  useEffect(() => {
    if (!team && teamsQuery.data?.[0]) {
      setTeam(teamsQuery.data[0].name)
    }
  }, [team, teamsQuery.data])

  const types = hierarchyQuery.data?.types ?? [...new Set(nodes.map((node) => node.type))]
  const states = [...new Set(nodes.map((node) => node.state))].sort()

  const overlay: OverlayFilter = {
    types: hiddenTypes.length ? types.filter((type) => !hiddenTypes.includes(type)) : null,
    states: hiddenStates.length ? states.filter((state) => !hiddenStates.includes(state)) : null,
    assignee,
    iterationPath: iterationPath || null,
    currentUserUniqueName: sessionQuery.data?.username
  }

  const visible = useMemo(() => applyOverlays(nodes, overlay), [nodes, overlay])

  const openForm = async (id: number) => {
    if (!scope) {
      return
    }
    try {
      const model = await window.planner.ado.form(scope.org, scope.project, id)
      setOpenId(id)
      setForm(model)
    } catch (error) {
      showErrorToast(error, 'Could not open Work Item')
    }
  }

  if (sessionQuery.isPending) {
    return (
      <div
        className="text-muted-foreground flex h-svh items-center justify-center gap-2 text-sm"
        data-testid="session-loading"
      >
        <Spinner />
        <p className="text-muted-foreground text-sm">Starting Session…</p>
      </div>
    )
  }

  if (!sessionQuery.data) {
    return (
      <div className="flex h-svh flex-col items-center justify-center gap-3">
        <p className="text-muted-foreground text-sm">Could not start Session.</p>
        <Button onClick={() => void sessionQuery.refetch()}>Retry</Button>
      </div>
    )
  }

  if (!sessionQuery.data.signedIn) {
    return <SignInShell session={sessionQuery.data} onLogin={(creds) => login.mutate(creds)} />
  }

  const sessionName = sessionQuery.data.displayName ?? 'Signed in'

  return (
    <div className="bg-background text-foreground flex h-svh min-h-0 flex-col">
      <header
        className="flex shrink-0 flex-wrap items-end gap-x-5 gap-y-3 border-b px-5 py-3.5"
        data-testid="signed-in-chrome"
      >
        <div className="flex min-w-0 items-center gap-3 pb-0.5">
          <div
            className="bg-primary/15 text-primary flex size-9 shrink-0 items-center justify-center rounded-full text-xs font-semibold"
            aria-hidden
          >
            {initials(sessionName)}
          </div>
          <div className="min-w-0">
            <p className="text-muted-foreground text-[11px] font-medium tracking-wider uppercase">
              ADO Planner
            </p>
            <p className="truncate text-sm font-medium" data-testid="session-name">
              {sessionName}
            </p>
          </div>
        </div>
        <div className="flex min-w-0 flex-1 flex-wrap items-end gap-x-3 gap-y-2">
          <ScopeField
            id="org"
            label="Org"
            value={org}
            placeholder="Select org"
            disabled={scopeIsFetching}
            loading={orgsQuery.isFetching}
            onChange={(value) => {
              setOrg(value)
              setProject('')
              setTeam('')
              setIterationPath('')
              setNodes([])
            }}
            options={(orgsQuery.data ?? []).map((row) => ({
              value: row.accountName,
              label: row.accountName
            }))}
          />
          <ScopeField
            id="project"
            label="Project"
            value={project}
            placeholder="Select project"
            disabled={!org || scopeIsFetching}
            loading={projectsQuery.isFetching}
            onChange={(value) => {
              setProject(value)
              setTeam('')
              setIterationPath('')
              setNodes([])
            }}
            options={(projectsQuery.data ?? []).map((row) => ({
              value: row.name,
              label: row.name
            }))}
          />
          <ScopeField
            id="team"
            label="Team"
            value={team}
            placeholder="Select Team"
            disabled={!project || scopeIsFetching}
            loading={teamsQuery.isFetching || hierarchyQuery.isFetching}
            onChange={(value) => {
              setTeam(value)
              setIterationPath('')
              setNodes([])
            }}
            options={(teamsQuery.data ?? []).map((row) => ({
              value: row.name,
              label: row.name
            }))}
          />
          <ScopeField
            id="iteration"
            label="Iteration"
            value={iterationPath}
            placeholder="All"
            disabled={!team || scopeIsFetching}
            loading={iterationsQuery.isFetching}
            onChange={setIterationPath}
            options={(iterationsQuery.data ?? []).map((row) => ({
              value: row.path,
              label: row.name
            }))}
          />
          <ScopeField
            id="assignee"
            label="Assignee"
            value={assignee}
            placeholder="Anyone"
            allowEmpty={false}
            onChange={(value) => setAssignee((value || 'anyone') as AssigneeFilter)}
            options={[
              { value: 'anyone', label: 'Anyone' },
              { value: 'me', label: 'Me' },
              { value: 'unassigned', label: 'Unassigned' }
            ]}
          />
        </div>
        <div className="ml-auto flex items-end gap-3 pb-0.5" data-testid="filters">
          <div className="flex flex-col gap-1.5">
            <span className="text-muted-foreground text-[11px] leading-none font-medium tracking-wide uppercase">
              Filters
            </span>
            <div className="flex items-center gap-2">
              <FilterMenu
                id="type-filter"
                label="Types"
                items={types}
                hidden={hiddenTypes}
                onChange={setHiddenTypes}
              />
              <FilterMenu
                id="state-filter"
                label="States"
                items={states}
                hidden={hiddenStates}
                onChange={setHiddenStates}
              />
            </div>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger
              render={<Button variant="outline" size="icon" aria-label="Appearance" id="flavor" />}
            >
              <PaletteIcon />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-40">
              <DropdownMenuGroup>
                <DropdownMenuLabel>Flavor</DropdownMenuLabel>
                <DropdownMenuRadioGroup
                  value={flavor}
                  onValueChange={(value) => setFlavor(value as Flavor)}
                >
                  {FLAVORS.map((name) => (
                    <DropdownMenuRadioItem key={name} value={name}>
                      {FLAVOR_LABELS[name]}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>
          <Button
            variant="outline"
            onClick={() => {
              void window.planner.session
                .logout()
                .then(() => sessionQuery.refetch())
                .catch((error: unknown) => showErrorToast(error, 'Could not log out'))
            }}
          >
            Log out
          </Button>
        </div>
      </header>
      <div className="bg-muted/25 min-h-0 flex-1 p-4">
        {scope ? (
          <div className="bg-card h-full min-h-0 overflow-hidden rounded-xl border shadow-sm">
            <HierarchyGantt
              scope={scope}
              items={visible}
              iterations={iterationsQuery.data ?? []}
              loading={hierarchyQuery.isFetching}
              onItemsChange={setNodes}
              onOpen={(id) => void openForm(id)}
            />
          </div>
        ) : (
          <div className="text-muted-foreground flex h-full flex-col items-center justify-center gap-1 text-sm">
            <p className="text-foreground font-medium">Choose a Team</p>
            <p>Pick an org, project, and Team to load the Hierarchy.</p>
          </div>
        )}
      </div>
      {form && openId && scope ? (
        <WorkItemFormDialog
          org={scope.org}
          project={scope.project}
          model={form}
          onClose={() => {
            setForm(null)
            setOpenId(null)
          }}
          onSaved={(rev, values) => {
            setNodes((prev) =>
              prev.map((row) =>
                row.id === openId
                  ? {
                      ...row,
                      rev,
                      title: String(values['System.Title'] ?? row.title),
                      state: String(values['System.State'] ?? row.state)
                    }
                  : row
              )
            )
            setForm(null)
            setOpenId(null)
          }}
        />
      ) : null}
      {updater ? (
        <Dialog open onOpenChange={() => undefined}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Update available</DialogTitle>
              <DialogDescription>
                Version {updater.version} is available. Download, install, and restart?
              </DialogDescription>
            </DialogHeader>
            {updater.releaseNotes ? <p className="text-sm">{updater.releaseNotes}</p> : null}
            <DialogFooter>
              <Button
                variant="outline"
                onClick={() => {
                  void window.planner.updater
                    .snooze()
                    .then(() => setUpdater(null))
                    .catch((error: unknown) => showErrorToast(error, 'Could not dismiss update'))
                }}
              >
                No
              </Button>
              <Button
                onClick={() => {
                  void window.planner.updater
                    .apply()
                    .catch((error: unknown) => showErrorToast(error, 'Could not apply update'))
                }}
              >
                Yes
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      ) : null}
    </div>
  )
}

export default function App() {
  return <PlannerApp />
}
