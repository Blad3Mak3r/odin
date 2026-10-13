import { Navigate, useParams } from 'react-router-dom'
import { GameBackdrop } from '@/components/GameBackdrop'
import { GameIcon } from '@/components/GameIcon'
import { CreateManagedInstanceDialog } from '@/components/instance/CreateManagedInstanceDialog'
import { ManagedInstancesTable } from '@/components/instance/ManagedInstancesTable'
import { QueryError } from '@/components/QueryError'
import { RouteTabs } from '@/components/RouteTabs'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { NativeConfigCodeEditor } from '@/components/instance/NativeConfigCodeEditor'
import { Skeleton } from '@/components/ui/skeleton'
import { ValheimModsTab } from '@/pages/GlobalModsPage'
import { GAME_DESCRIPTIONS } from '@/lib/games'
import { useArkClusters, useCreateArkCluster, useDeleteArkCluster, useGames, useInitializeSharedAdvancedConfig, useInstallShared, useManagedInstances, useSetArkClusterMembers, useSharedAdvancedConfig, useSharedInstall, useSharedRawAdvancedConfig, useUpdateSharedAdvancedConfig, useUpdateSharedRawAdvancedConfig } from '@/lib/queries'
import { useState } from 'react'
import type { GameView } from '@/lib/types'
import { toast } from 'sonner'

function RuntimeCard({ game }: { game: GameView }) {
  return <Card><CardHeader><CardTitle>Per-instance runtime</CardTitle><CardDescription>{game.name} is installed and updated inside each running Podman runtime. Create an instance, start it, then use its Update action.</CardDescription></CardHeader></Card>
}

function SharedInstallCard({ game }: { game: GameView }) {
  const install = useSharedInstall(game.id, true)
  const start = useInstallShared(game.id)
  if (install.isLoading) return <Card><CardHeader><CardTitle>Shared installation</CardTitle><CardDescription>Checking the shared {game.name} payload…</CardDescription></CardHeader></Card>
  if (install.isError) return <Card><CardHeader><CardTitle>Shared installation</CardTitle><CardDescription>Its status could not be loaded.</CardDescription></CardHeader></Card>
  const state = install.data
  return <Card><CardHeader><CardTitle>Shared installation</CardTitle><CardDescription>One {game.name} payload can be reused by shared instances. Stop every shared instance before installing or updating it.</CardDescription></CardHeader><CardContent className="flex flex-wrap items-center gap-3"><p className="text-sm text-muted-foreground">{state?.running ? 'Installing…' : state?.installed ? `Installed${state.build_id ? ` · Steam build ${state.build_id}` : ''}` : 'Not installed'}</p><Button type="button" disabled={state?.running || start.isPending} onClick={() => start.mutate(undefined, { onSuccess: () => toast.success(`Shared ${game.name} installation started`), onError: (error) => toast.error(error.message) })}>{state?.installed ? 'Update shared install' : 'Install shared files'}</Button></CardContent></Card>
}

function ArkSharedConfigCard() {
  const config = useSharedAdvancedConfig('ark-asa', true)
  const initialize = useInitializeSharedAdvancedConfig('ark-asa')
  const update = useUpdateSharedAdvancedConfig('ark-asa')
  const instances = useManagedInstances()
  const [values, setValues] = useState<Record<string, string>>({})
  const running = instances.data?.some((instance) => instance.game === 'ark-asa' && instance.install_mode === 'shared' && instance.running) ?? false
  if (config.isLoading) return <Card><CardHeader><CardTitle>Shared configuration</CardTitle><CardDescription>Loading native ARK configuration…</CardDescription></CardHeader></Card>
  if (config.isError) return <Card><CardHeader><CardTitle>Shared configuration</CardTitle><CardDescription>Configuration could not be loaded.</CardDescription></CardHeader></Card>
  const files = config.data?.files ?? []
  const save = () => {
    const changes = Object.entries(values).map(([id, value]) => {
      const [file, ...key] = id.split(':')
      return { file, key: key.join(':'), value }
    })
    if (changes.length) update.mutate(changes, { onSuccess: () => { setValues({}); toast.success('Shared ARK configuration saved') }, onError: (error) => toast.error(error.message) })
  }
  return <Card><CardHeader><CardTitle>Shared configuration</CardTitle><CardDescription>These native ARK INI files apply to every shared-install server. Stop all shared servers before saving.</CardDescription></CardHeader><CardContent className="flex flex-col gap-5">{files.some((file) => !file.exists) && <Button className="w-fit" type="button" variant="outline" disabled={running || initialize.isPending} onClick={() => initialize.mutate(undefined, { onSuccess: () => toast.success('Shared configuration initialized'), onError: (error) => toast.error(error.message) })}>Initialize configuration files</Button>}{files.filter((file) => file.exists).map((file) => <div key={file.id} className="border"><div className="border-b px-4 py-3"><p className="text-sm font-medium">{file.path}</p></div><div className="grid gap-3 p-4 sm:grid-cols-2">{file.sections.flatMap((section) => section.entries).map((entry) => { const id = `${file.id}:${entry.key}`; return <div key={id} className="flex flex-col gap-1"><Label htmlFor={id}>{entry.key}</Label><Input id={id} type={entry.sensitive ? 'password' : 'text'} defaultValue={entry.value} placeholder={entry.sensitive && entry.configured ? '••••••••' : undefined} disabled={running || entry.managed} onChange={(event) => setValues((current) => ({ ...current, [id]: event.target.value }))} /></div> })}</div><details className="border-t"><summary className="cursor-pointer px-4 py-3 text-sm font-medium">Edit source</summary><div className="p-4"><ArkSharedConfigCodeFile file={file.id} running={running} /></div></details></div>)}<Button className="w-fit" type="button" disabled={running || update.isPending || Object.keys(values).length === 0} onClick={save}>Save shared configuration</Button></CardContent></Card>
}

function ArkSharedConfigCodeFile({ file, running }: { file: string; running: boolean }) {
  const source = useSharedRawAdvancedConfig('ark-asa', file, true)
  const save = useUpdateSharedRawAdvancedConfig('ark-asa', file)
  if (source.isLoading) return <p className="text-sm text-muted-foreground">Loading source…</p>
  if (source.isError || !source.data) return <p className="text-sm text-destructive">Source could not be loaded.</p>
  return <NativeConfigCodeEditor key={`${file}:${source.data.content}`} content={source.data.content} format={source.data.format} disabled={running} saving={save.isPending} onSave={(content) => save.mutate(content, { onSuccess: () => toast.success('Configuration source saved'), onError: (error) => toast.error(error.message) })} />
}

function ArkClustersCard() {
  const clusters = useArkClusters()
  const instances = useManagedInstances()
  const create = useCreateArkCluster()
  const remove = useDeleteArkCluster()
  const setMembers = useSetArkClusterMembers()
  const [name, setName] = useState('')
  const [clusterId, setClusterId] = useState('')
  const arkInstances = instances.data?.filter((instance) => instance.game === 'ark-asa') ?? []
  const submit = () => create.mutate({ name, cluster_id: clusterId }, { onSuccess: () => { setName(''); setClusterId(''); toast.success('ARK cluster created') }, onError: (error) => toast.error(error.message) })
  return <Card><CardHeader><CardTitle>Clusters</CardTitle><CardDescription>Members share Cross-ARK transfer storage. Stop all affected servers before changing membership.</CardDescription></CardHeader><CardContent className="flex flex-col gap-4"><div className="grid gap-3 sm:grid-cols-3"><Input placeholder="Cluster name" value={name} onChange={(event) => setName(event.target.value)} /><Input placeholder="Cluster ID" value={clusterId} onChange={(event) => setClusterId(event.target.value)} /><Button type="button" disabled={!name || !clusterId || create.isPending} onClick={submit}>Create cluster</Button></div>{clusters.data?.map((cluster) => <div key={cluster.id} className="border p-4"><div className="flex items-baseline justify-between gap-4"><p className="font-medium">{cluster.name}</p><div className="flex items-center gap-3"><p className="text-xs text-muted-foreground">ID: {cluster.cluster_id}</p><Button type="button" variant="outline" size="sm" disabled={remove.isPending} onClick={() => { if (window.confirm(`Delete cluster ${cluster.name}?`)) remove.mutate(cluster.id, { onSuccess: () => toast.success('ARK cluster deleted'), onError: (error) => toast.error(error.message) }) }}>Delete</Button></div></div><div className="mt-3 grid gap-2">{arkInstances.map((instance) => <label key={instance.id} className="flex items-center gap-2 text-sm"><input type="checkbox" checked={cluster.members.includes(instance.id)} disabled={instance.running || setMembers.isPending} onChange={(event) => { const members = event.target.checked ? [...cluster.members, instance.id] : cluster.members.filter((id) => id !== instance.id); setMembers.mutate({ id: cluster.id, members }, { onError: (error) => toast.error(error.message) }) }} />{instance.name}{instance.running ? ' (running)' : ''}</label>)}</div></div>)}</CardContent></Card>
}

function GameInstances({ game }: { game: GameView }) {
  const instances = useManagedInstances()
  const gameInstances = instances.data?.filter((instance) => instance.game === game.id) ?? []

  return (
    <ManagedInstancesTable
      instances={gameInstances}
      isLoading={instances.isLoading}
      error={instances.isError ? instances.error : undefined}
      emptyMessage={`No ${game.name} instances yet.`}
      withGame={false}
    />
  )
}

function GameHeader({ game, games }: { game: GameView; games: GameView[] }) {
  const instances = useManagedInstances()
  const mine = instances.data?.filter((instance) => instance.game === game.id)
  const running = mine?.filter((instance) => instance.running).length ?? 0

  return (
    <header className="flex flex-col gap-4 pt-2 sm:flex-row sm:items-end sm:justify-between">
      <div className="flex items-center gap-4 sm:gap-5">
        <GameIcon game={game.id} className="size-16 sm:size-20" />
        <div>
          <h1 className="text-3xl font-semibold tracking-tight">{game.name}</h1>
          <p className="mt-2 max-w-2xl text-sm text-foreground/80">{GAME_DESCRIPTIONS[game.id]}</p>
          {mine && (
            <p className="mt-1 text-sm tabular-nums text-foreground/80">
              {mine.length} {mine.length === 1 ? 'instance' : 'instances'} · {running} running
            </p>
          )}
        </div>
      </div>
      <CreateManagedInstanceDialog games={games} defaultGame={game.id} />
    </header>
  )
}

function hasGlobalMods(game: GameView) {
  return game.id === 'valheim' && game.capabilities.mods
}

export function GameDetailPage() {
  const { game: gameParam, '*': tabPath } = useParams<{ game: string; '*': string }>()
  const games = useGames()
  const game = games.data?.find((candidate) => candidate.id === gameParam)
  const [tab, ...nestedPath] = tabPath?.split('/').filter(Boolean) ?? []
  const activeTab = tab || 'overview'

  if (games.isError) return <QueryError error={games.error} />
  if (games.isLoading) return <Skeleton className="h-56" />
  if (!game) return <Navigate replace to="/games" />
  if (tab === 'overview' && nestedPath.length === 0) return <Navigate replace to={`/games/${game.id}`} />
  const hasSharedConfig = game.id === 'ark-asa'
  if (!['overview', 'instances', 'mods', 'config'].includes(activeTab) || (nestedPath.length > 0 && activeTab !== 'mods') || (activeTab === 'mods' && !hasGlobalMods(game)) || (activeTab === 'config' && !hasSharedConfig)) {
    return <Navigate replace to={`/games/${game.id}`} />
  }

  return (
    <div className="relative isolate flex flex-col gap-6">
      <GameBackdrop game={game.id} />
      <GameHeader game={game} games={games.data ?? []} />
      <RouteTabs
        label={`${game.name} sections`}
        tabs={[
          { label: 'Overview', to: `/games/${game.id}`, end: true },
          { label: 'Instances', to: `/games/${game.id}/instances` },
          ...(hasSharedConfig ? [{ label: 'Configuration', to: `/games/${game.id}/config` }] : []),
          ...(hasGlobalMods(game) ? [{ label: 'Mods', to: `/games/${game.id}/mods` }] : []),
        ]}
      />
      {activeTab === 'overview' && (game.capabilities.shared_install ? <><SharedInstallCard game={game} />{game.id === 'ark-asa' && <ArkClustersCard />}</> : <RuntimeCard game={game} />)}
      {activeTab === 'instances' && <GameInstances game={game} />}
      {activeTab === 'config' && <ArkSharedConfigCard />}
      {activeTab === 'mods' && <ValheimModsTab path={nestedPath} />}
    </div>
  )
}
