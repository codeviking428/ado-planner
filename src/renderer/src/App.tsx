import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from '@/components/ui/dialog'
import { useFlavor } from '@/components/flavor-provider'
import { HierarchyGantt } from '@/components/hierarchy-gantt'
import { WorkItemFormDialog } from '@/components/work-item-form'
import { applyOverlays } from '@shared/overlays'
import { FLAVORS } from '@shared/flavor'
import type {
  AssigneeFilter,
  OverlayFilter,
  ScopeSelection,
  SessionInfo,
  UpdaterPrompt,
  WorkItemFormModel,
  WorkItemNode
} from '@shared/types'

function NativeSelect({
  id,
  value,
  onChange,
  disabled,
  children,
  label
}: {
  id: string
  value: string
  onChange: (value: string) => void
  disabled?: boolean
  children: ReactNode
  label: string
}) {
  return (
    <label className="flex items-center gap-2 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <select
        id={id}
        className="border-input bg-background h-8 rounded-md border px-2 text-sm"
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      >
        {children}
      </select>
    </label>
  )
}

function SignInShell({
  session,
  onLogin
}: {
  session: SessionInfo | undefined
  onLogin: () => void
}) {
  return (
    <div
      className="flex h-svh flex-col items-center justify-center gap-4"
      data-testid="sign-in-shell"
    >
      <h1 className="text-xl font-medium">ADO Planner</h1>
      <p className="text-muted-foreground max-w-md text-center text-sm">
        Sign in with your work or school account to view a Team Work Item Hierarchy on a Gantt.
      </p>
      <Button onClick={onLogin} data-testid="sign-in">
        Sign in
      </Button>
      {session && !session.signedIn ? (
        <p className="text-muted-foreground text-xs">No Session yet.</p>
      ) : null}
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
    mutationFn: () => window.planner.session.login(),
    onSuccess: (info) => {
      sessionQuery.refetch()
      if (!info.signedIn) {
        toast.error('Sign-in did not create a Session')
      }
    },
    onError: (error: Error) => toast.error(error.message)
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

  useEffect(() => {
    if (hierarchyQuery.data) {
      setNodes(hierarchyQuery.data.nodes)
      if (hierarchyQuery.data.truncated) {
        toast.warning('Hierarchy is incomplete — WIQL hit the 20,000 result cap.')
      }
    }
  }, [hierarchyQuery.data])

  useEffect(() => {
    if (hierarchyQuery.error) {
      toast.error(
        hierarchyQuery.error instanceof Error
          ? hierarchyQuery.error.message
          : 'Could not load Hierarchy'
      )
    }
  }, [hierarchyQuery.error])

  useEffect(() => {
    const offAvailable = window.planner.updater.onAvailable(setUpdater)
    const offError = window.planner.updater.onError((message) => toast.error(message))
    void window.planner.updater.prompt().then((prompt) => {
      if (prompt) {
        setUpdater(prompt)
      }
    })
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
      toast.error(error instanceof Error ? error.message : 'Could not open Work Item')
    }
  }

  if (!sessionQuery.data?.signedIn) {
    return <SignInShell session={sessionQuery.data} onLogin={() => login.mutate()} />
  }

  return (
    <div className="bg-background text-foreground flex h-svh min-h-0 flex-col">
      <header
        className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b px-4 py-2"
        data-testid="signed-in-chrome"
      >
        <div className="min-w-0">
          <p className="text-muted-foreground text-xs tracking-wide uppercase">ADO Planner</p>
          <p className="text-sm" data-testid="session-name">
            {sessionQuery.data.displayName}
          </p>
        </div>
        <NativeSelect
          id="org"
          label="Org"
          value={org}
          onChange={(value) => {
            setOrg(value)
            setProject('')
            setTeam('')
          }}
        >
          <option value="">Select org</option>
          {(orgsQuery.data ?? []).map((row) => (
            <option key={row.accountName} value={row.accountName}>
              {row.accountName}
            </option>
          ))}
        </NativeSelect>
        <NativeSelect
          id="project"
          label="Project"
          value={project}
          onChange={(value) => {
            setProject(value)
            setTeam('')
          }}
          disabled={!org}
        >
          <option value="">Select project</option>
          {(projectsQuery.data ?? []).map((row) => (
            <option key={row.id} value={row.name}>
              {row.name}
            </option>
          ))}
        </NativeSelect>
        <NativeSelect id="team" label="Team" value={team} onChange={setTeam} disabled={!project}>
          <option value="">Select Team</option>
          {(teamsQuery.data ?? []).map((row) => (
            <option key={row.id} value={row.name}>
              {row.name}
            </option>
          ))}
        </NativeSelect>
        <NativeSelect
          id="iteration"
          label="Iteration"
          value={iterationPath}
          onChange={setIterationPath}
          disabled={!team}
        >
          <option value="">All</option>
          {(iterationsQuery.data ?? []).map((row) => (
            <option key={row.path} value={row.path}>
              {row.name}
            </option>
          ))}
        </NativeSelect>
        <NativeSelect
          id="assignee"
          label="Assignee"
          value={assignee}
          onChange={(value) => setAssignee(value as AssigneeFilter)}
        >
          <option value="anyone">Anyone</option>
          <option value="me">Me</option>
          <option value="unassigned">Unassigned</option>
        </NativeSelect>
        <NativeSelect
          id="flavor"
          label="Flavor"
          value={flavor}
          onChange={(value) => setFlavor(value as (typeof FLAVORS)[number])}
        >
          {FLAVORS.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </NativeSelect>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            void window.planner.session.logout().then(() => sessionQuery.refetch())
          }}
        >
          Log out
        </Button>
      </header>
      <div className="flex gap-2 border-b px-4 py-2 text-xs" data-testid="filters">
        <span className="text-muted-foreground">Hide types:</span>
        {types.map((type) => (
          <label key={type} className="flex items-center gap-1">
            <input
              type="checkbox"
              checked={hiddenTypes.includes(type)}
              onChange={(event) =>
                setHiddenTypes((prev) =>
                  event.target.checked ? [...prev, type] : prev.filter((row) => row !== type)
                )
              }
            />
            {type}
          </label>
        ))}
        <span className="text-muted-foreground ml-4">Hide states:</span>
        {states.map((state) => (
          <label key={state} className="flex items-center gap-1">
            <input
              type="checkbox"
              checked={hiddenStates.includes(state)}
              onChange={(event) =>
                setHiddenStates((prev) =>
                  event.target.checked ? [...prev, state] : prev.filter((row) => row !== state)
                )
              }
            />
            {state}
          </label>
        ))}
      </div>
      <div className="min-h-0 flex-1 p-3">
        {scope ? (
          <HierarchyGantt
            scope={scope}
            items={visible}
            onItemsChange={setNodes}
            onOpen={(id) => void openForm(id)}
          />
        ) : (
          <div className="text-muted-foreground flex h-full items-center justify-center text-sm">
            Pick org, project, and Team.
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
                  void window.planner.updater.snooze()
                  setUpdater(null)
                }}
              >
                No
              </Button>
              <Button
                onClick={() => {
                  void window.planner.updater.apply()
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
