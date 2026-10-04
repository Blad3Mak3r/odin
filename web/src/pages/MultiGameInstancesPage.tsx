import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api-client'
import { formatBytes } from '@/lib/utils'
import { Checkbox } from '@/components/ui/checkbox'
import { ManageInstanceDialog } from '@/components/instance/ManageInstanceDialog'
import { GameIcon } from '@/components/GameIcon'
import { Loader2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router-dom'
import { toast } from 'sonner'
import { PageHeader } from '@/components/PageHeader'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { QueryError } from '@/components/QueryError'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { useCreateManagedInstance, useGameInstallStatus, useGames, useInstallGame, useJobs, useManagedInstanceAction, useManagedInstanceTransition, useManagedInstances } from '@/lib/queries'
import type { GameId, GameView, ManagedInstanceView, InstanceResources } from '@/lib/types'

type Filter = 'all' | GameId

export function MultiGameInstancesPage() {
  const instances = useManagedInstances()
  const games = useGames()
  const [filter, setFilter] = useState<Filter>('all')
  const [search, setSearch] = useState('')
  const [sort, setSort] = useState('name')
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const client = useQueryClient()
  const visible = (instances.data ?? []).filter((instance) => (filter === 'all' || instance.game === filter) && [instance.name, ...instance.tags].join(' ').toLowerCase().includes(search.toLowerCase())).sort((a, b) => sort === 'game' ? a.game.localeCompare(b.game) || a.name.localeCompare(b.name) : sort === 'status' ? Number(b.running) - Number(a.running) || a.name.localeCompare(b.name) : a.name.localeCompare(b.name))
  const targets = (instances.data ?? []).filter((instance) => selected.has(instance.id))
  const [results, setResults] = useState<{ game: GameId; name: string; ok: boolean; error: string | null }[]>([])
  const bulk = useMutation({
    mutationFn: (operation: string) => api.post<typeof results>(`/games/instances/bulk/${operation}`, { instances: targets.map(({ game, name }) => ({ game, name })) }),
    onSuccess: (data) => { setResults(data); setSelected(new Set()); client.invalidateQueries({ queryKey: ['managed-instances'] }); client.invalidateQueries({ queryKey: ['jobs'] }) },
    onError: (error) => toast.error(error.message),
  })
  const toggle = (id: string) => setSelected((previous) => { const next = new Set(previous); if (next.has(id)) next.delete(id); else next.add(id); return next })

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Instances"
        description="Every game server Odin manages."
        action={<CreateManagedInstanceDialog />}
      />
      <ToggleGroup value={[filter]} onValueChange={(value) => value[0] && setFilter(value[0] as Filter)} variant="outline" size="sm">
        <ToggleGroupItem value="all">All</ToggleGroupItem>
        <ToggleGroupItem value="valheim"><GameIcon game="valheim" />Valheim</ToggleGroupItem>
        <ToggleGroupItem value="rust"><GameIcon game="rust" />Rust</ToggleGroupItem>
      </ToggleGroup>
      {games.isError && <QueryError error={games.error} />}
      {games.data && (
        <section className="grid gap-4 md:grid-cols-2">
          {games.data.map((game) => <GameInstallCard key={game.id} game={game} />)}
        </section>
      )}
      <div className="flex flex-wrap gap-3">
        <Input aria-label="Search instances or tags" placeholder="Search instances or tags…" value={search} onChange={(e) => setSearch(e.target.value)} className="max-w-sm" />
        <select aria-label="Sort instances" className="rounded-md border bg-background px-3 text-sm" value={sort} onChange={(e) => setSort(e.target.value)}><option value="name">Name</option><option value="game">Game</option><option value="status">Running first</option></select>
      </div>
      {targets.length > 0 && <div className="flex flex-wrap items-center gap-2 rounded-xl border p-3">
        <span className="text-sm">{targets.length} selected</span>
        {['start', 'stop', 'restart', ...(targets.every((target) => target.capabilities.mods) ? ['mods', 'bepinex'] : [])].map((operation) => <Button key={operation} size="sm" variant="outline" disabled={bulk.isPending || targets.length > 100} onClick={() => bulk.mutate(operation)}>{operation === 'mods' ? 'Update mods' : operation === 'bepinex' ? 'Update BepInEx' : operation}</Button>)}
        <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>Clear selection</Button>
      </div>}
      {results.length > 0 && <div className="text-sm" role="status">{results.map((result) => <p key={`${result.game}/${result.name}`} className={result.ok ? 'text-muted-foreground' : 'text-destructive'}>{result.game} / {result.name}: {result.ok ? 'Operation accepted' : result.error}</p>)}</div>}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead><Checkbox aria-label="Select visible instances" checked={visible.length > 0 && visible.every((i) => selected.has(i.id))} onCheckedChange={(checked) => setSelected((previous) => { const next = new Set(previous); for (const instance of visible) { if (checked) next.add(instance.id); else next.delete(instance.id) } return next })} /></TableHead><TableHead>Name</TableHead>
            <TableHead>Game</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="hidden sm:table-cell">Port</TableHead>
            <TableHead className="hidden lg:table-cell">CPU / RAM</TableHead><TableHead className="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {instances.isLoading && <LoadingRows />}
          {instances.isError && <TableRow><TableCell colSpan={7}><QueryError error={instances.error} /></TableCell></TableRow>}
          {!instances.isLoading && !instances.isError && visible.length === 0 && (
            <TableRow><TableCell colSpan={7} className="text-center text-muted-foreground">No instances for this filter.</TableCell></TableRow>
          )}
          {visible.map((instance) => <ManagedInstanceRow key={instance.id} instance={instance} selected={selected.has(instance.id)} onToggle={() => toggle(instance.id)} />)}
        </TableBody>
      </Table>
    </div>
  )
}

export function GameInstallCard({ game }: { game: GameView }) {
  const status = useGameInstallStatus(game.id)
  const install = useInstallGame()
  const jobs = useJobs()
  const wasRunning = useRef(false)
  const runningJob = jobs.data?.find(
    (job) => job.kind.kind === 'steamcmd_install' && job.kind.game === game.id &&
      (job.status.status === 'queued' || job.status.status === 'running'),
  )

  useEffect(() => {
    if (wasRunning.current && !runningJob) status.refetch()
    wasRunning.current = Boolean(runningJob)
  }, [runningJob, status])

  const label = !status.data?.installed
    ? 'Not installed'
    : status.data.update_available
      ? `Update available: ${status.data.installed_build_id} → ${status.data.latest_build_id}`
      : `Up to date (build ${status.data.installed_build_id})`

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between space-y-0">
        <div>
          <CardTitle className="flex items-center gap-2 text-base"><GameIcon game={game.id} className="size-7 rounded-md" />{game.name}</CardTitle>
          <CardDescription>Steam App ID {game.steam_app_id}</CardDescription>
        </div>
        <Button
          size="sm"
          disabled={install.isPending || Boolean(runningJob)}
          onClick={() => install.mutate(game.id, { onError: (error) => toast.error(error.message) })}
        >
          {(install.isPending || runningJob) && <Loader2 className="size-4 animate-spin" />}
          Install / update
        </Button>
      </CardHeader>
      <CardContent>
        {status.isError ? <QueryError error={status.error} /> : <Badge variant={status.data?.update_available ? 'secondary' : 'outline'}>{label}</Badge>}
      </CardContent>
    </Card>
  )
}

function LoadingRows() {
  return Array.from({ length: 3 }, (_, index) => (
    <TableRow key={index}><TableCell colSpan={7}><Skeleton className="h-5 w-full" /></TableCell></TableRow>
  ))
}

function ManagedInstanceRow({ instance, selected, onToggle }: { instance: ManagedInstanceView; selected: boolean; onToggle: () => void }) {
  const resources = useQuery({ queryKey: instance.game === 'valheim' ? ['resources', 'instance', instance.name] : ['managed-instances', 'rust', instance.name, 'resources'], queryFn: () => api.get<InstanceResources>(`/games/${instance.game}/instances/${instance.name}/resources`), enabled: instance.running, refetchInterval: 10_000 })
  const start = useManagedInstanceAction('start')
  const stop = useManagedInstanceAction('stop')
  const restart = useManagedInstanceAction('restart')
  const transition = useManagedInstanceTransition(instance.game, instance.name)
  const busy = start.isPending || stop.isPending || restart.isPending || transition.data !== null
  const port = typeof instance.config.port === 'number' ? instance.config.port : '—'
  const action = { game: instance.game, name: instance.name }

  return (
    <TableRow>
      <TableCell><Checkbox aria-label={`Select ${instance.game} / ${instance.name}`} checked={selected} onCheckedChange={onToggle} /></TableCell>
      <TableCell className="font-medium"><Link className="hover:underline" to={`/instances/${instance.game}/${instance.name}`}>{instance.name}</Link><div className="flex flex-wrap gap-1">{instance.tags.map((tag) => <Badge key={tag} variant="outline">{tag}</Badge>)}</div></TableCell>
      <TableCell><Badge variant="secondary"><GameIcon game={instance.game} className="size-4 rounded-sm" />{instance.game}</Badge></TableCell>
      <TableCell><Badge variant={instance.running ? 'default' : 'secondary'}>{instance.running ? 'running' : 'stopped'}</Badge></TableCell>
      <TableCell className="hidden sm:table-cell">{port}</TableCell>
      <TableCell className="hidden lg:table-cell">{instance.running && resources.data ? `${resources.data.cpu_percent.toFixed(0)}% · ${formatBytes(resources.data.memory_bytes)}` : '—'}</TableCell>
      <TableCell className="text-right"><div className="flex flex-wrap justify-end gap-2"><ManageInstanceDialog instance={instance} />
        {instance.running ? (
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="outline" disabled={busy} onClick={() => restart.mutate(action, { onError: (error) => toast.error(error.message) })}>Restart</Button>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => stop.mutate(action, { onError: (error) => toast.error(error.message) })}>Stop</Button>
          </div>
        ) : <Button size="sm" disabled={busy} onClick={() => start.mutate(action, { onError: (error) => toast.error(error.message) })}>Start</Button>}
      </div></TableCell>
    </TableRow>
  )
}

function CreateManagedInstanceDialog() {
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [game, setGame] = useState<GameId>('valheim')
  const create = useCreateManagedInstance()
  const submit = () => create.mutate({ game, name }, {
    onSuccess: () => {
      setOpen(false)
      setName('')
      toast.success(`${game} instance '${name}' created`)
    },
    onError: (error) => toast.error(error.message),
  })

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button>New instance</Button>} />
      <DialogContent>
        <DialogHeader><DialogTitle>Create instance</DialogTitle></DialogHeader>
        <FieldGroup>
          <Field>
            <FieldLabel>Game</FieldLabel>
            <ToggleGroup value={[game]} onValueChange={(value) => value[0] && setGame(value[0] as GameId)} variant="outline" spacing={0}>
              <ToggleGroupItem value="valheim"><GameIcon game="valheim" />Valheim</ToggleGroupItem>
              <ToggleGroupItem value="rust"><GameIcon game="rust" />Rust</ToggleGroupItem>
            </ToggleGroup>
          </Field>
          <Field>
            <FieldLabel htmlFor="managed-instance-name">Name</FieldLabel>
            <Input id="managed-instance-name" placeholder="my-server" value={name} onChange={(event) => setName(event.target.value)} onKeyDown={(event) => event.key === 'Enter' && name && submit()} />
          </Field>
        </FieldGroup>
        <DialogFooter><Button disabled={!name || create.isPending} onClick={submit}>Create</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
