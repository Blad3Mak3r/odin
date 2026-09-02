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
import { useCreateManagedInstance, useGameInstallStatus, useGames, useInstallGame, useJobs, useManagedInstanceAction, useManagedInstances } from '@/lib/queries'
import type { GameId, GameView, ManagedInstanceView } from '@/lib/types'

type Filter = 'all' | GameId

export function MultiGameInstancesPage() {
  const instances = useManagedInstances()
  const games = useGames()
  const [filter, setFilter] = useState<Filter>('all')
  const visible = instances.data?.filter((instance) => filter === 'all' || instance.game === filter) ?? []

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Instances"
        description="Every game server Odin manages."
        action={<CreateManagedInstanceDialog />}
      />
      <ToggleGroup value={[filter]} onValueChange={(value) => value[0] && setFilter(value[0] as Filter)} variant="outline" size="sm">
        <ToggleGroupItem value="all">All</ToggleGroupItem>
        <ToggleGroupItem value="valheim">Valheim</ToggleGroupItem>
        <ToggleGroupItem value="rust">Rust</ToggleGroupItem>
      </ToggleGroup>
      {games.isError && <QueryError error={games.error} />}
      {games.data && (
        <section className="grid gap-4 md:grid-cols-2">
          {games.data.map((game) => <GameInstallCard key={game.id} game={game} />)}
        </section>
      )}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Game</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="hidden sm:table-cell">Port</TableHead>
            <TableHead className="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {instances.isLoading && <LoadingRows />}
          {instances.isError && <TableRow><TableCell colSpan={5}><QueryError error={instances.error} /></TableCell></TableRow>}
          {!instances.isLoading && !instances.isError && visible.length === 0 && (
            <TableRow><TableCell colSpan={5} className="text-center text-muted-foreground">No instances for this filter.</TableCell></TableRow>
          )}
          {visible.map((instance) => <ManagedInstanceRow key={instance.id} instance={instance} />)}
        </TableBody>
      </Table>
    </div>
  )
}

function GameInstallCard({ game }: { game: GameView }) {
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
          <CardTitle className="text-base">{game.name}</CardTitle>
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
    <TableRow key={index}><TableCell colSpan={5}><Skeleton className="h-5 w-full" /></TableCell></TableRow>
  ))
}

function ManagedInstanceRow({ instance }: { instance: ManagedInstanceView }) {
  const start = useManagedInstanceAction('start')
  const stop = useManagedInstanceAction('stop')
  const restart = useManagedInstanceAction('restart')
  const busy = start.isPending || stop.isPending || restart.isPending
  const port = typeof instance.config.port === 'number' ? instance.config.port : '—'
  const action = { game: instance.game, name: instance.name }

  return (
    <TableRow>
      <TableCell className="font-medium"><Link className="hover:underline" to={`/instances/${instance.game}/${instance.name}`}>{instance.name}</Link></TableCell>
      <TableCell><Badge variant="secondary">{instance.game}</Badge></TableCell>
      <TableCell><Badge variant={instance.running ? 'default' : 'secondary'}>{instance.running ? 'running' : 'stopped'}</Badge></TableCell>
      <TableCell className="hidden sm:table-cell">{port}</TableCell>
      <TableCell className="text-right">
        {instance.running ? (
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="outline" disabled={busy} onClick={() => restart.mutate(action, { onError: (error) => toast.error(error.message) })}>Restart</Button>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => stop.mutate(action, { onError: (error) => toast.error(error.message) })}>Stop</Button>
          </div>
        ) : <Button size="sm" disabled={busy} onClick={() => start.mutate(action, { onError: (error) => toast.error(error.message) })}>Start</Button>}
      </TableCell>
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
              <ToggleGroupItem value="valheim">Valheim</ToggleGroupItem>
              <ToggleGroupItem value="rust">Rust</ToggleGroupItem>
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
