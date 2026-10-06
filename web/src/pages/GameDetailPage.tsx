import { Loader2 } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { NavLink, Navigate, useParams } from 'react-router-dom'
import { toast } from 'sonner'
import palworldBanner from '@/assets/game-banners/palworld.webp'
import runescapeDragonwildsBanner from '@/assets/game-banners/runescape-dragonwilds.webp'
import rustBanner from '@/assets/game-banners/rust.webp'
import valheimBanner from '@/assets/game-banners/valheim.webp'
import vrisingBanner from '@/assets/game-banners/vrising.webp'
import { GameIcon } from '@/components/GameIcon'
import { ManagedInstancesTable } from '@/components/instance/ManagedInstancesTable'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { ValheimModsTab } from '@/pages/GlobalModsPage'
import { useGameInstallStatus, useGames, useInstallGame, useJobs, useManagedInstances } from '@/lib/queries'
import type { GameId, GameView } from '@/lib/types'
import { cn } from '@/lib/utils'

const GAME_DESCRIPTIONS: Record<GameId, string> = {
  valheim: 'Build, explore, and survive with a dedicated Valheim server.',
  rust: 'Host and manage your Rust dedicated servers.',
  vrising: 'Run your V Rising server and manage its world.',
  palworld: 'Operate your Palworld dedicated server from one place.',
  'runescape-dragonwilds': 'Host your RuneScape: Dragonwilds server.',
  '7d2d': 'Run and manage your 7 Days to Die dedicated servers.',
}

const GAME_BANNERS: Record<GameId, string> = {
  valheim: valheimBanner,
  rust: rustBanner,
  vrising: vrisingBanner,
  palworld: palworldBanner,
  'runescape-dragonwilds': runescapeDragonwildsBanner,
  '7d2d': '',
}

function InstallStatusCard({ game }: { game: GameView }) {
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
          <CardTitle>Installation</CardTitle>
          <CardDescription>Steam App ID {game.steam_app_id}</CardDescription>
        </div>
        <Button
          disabled={install.isPending || Boolean(runningJob)}
          onClick={() => install.mutate(game.id, { onError: (error) => toast.error(error.message) })}
        >
          {(install.isPending || runningJob) && <Loader2 className="size-4 animate-spin" />}
          Install / update
        </Button>
      </CardHeader>
      <CardContent>
        {status.isError
          ? <QueryError error={status.error} />
          : <Badge variant={status.data?.update_available ? 'secondary' : 'outline'}>{label}</Badge>}
      </CardContent>
    </Card>
  )
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

function GameHeader({ game }: { game: GameView }) {
  return (
    <section
      className="relative overflow-hidden rounded-2xl border bg-cover bg-center p-6 sm:p-8"
      style={{ backgroundImage: `linear-gradient(90deg, var(--card), color-mix(in oklab, var(--card) 72%, transparent), transparent), url(${GAME_BANNERS[game.id]})` }}
    >
      <div className="absolute inset-0 bg-gradient-to-br from-primary/10 via-transparent to-transparent" />
      <div className="relative flex items-center gap-5">
        <GameIcon game={game.id} className="size-20 rounded-2xl shadow-lg" />
        <div>
          <h1 className="text-3xl font-semibold tracking-tight">{game.name}</h1>
          <p className="mt-2 max-w-2xl text-sm text-muted-foreground">{GAME_DESCRIPTIONS[game.id]}</p>
        </div>
      </div>
    </section>
  )
}

function GameTabs({ game }: { game: GameView }) {
  const tabs = [
    { label: 'Overview', to: `/games/${game.id}`, end: true },
    { label: 'Instances', to: `/games/${game.id}/instances`, end: false },
    ...(game.capabilities.mods ? [{ label: 'Mods', to: `/games/${game.id}/mods`, end: false }] : []),
  ]

  return (
    <nav aria-label={`${game.name} sections`} className="overflow-x-auto">
      <div className="inline-flex h-8 items-center rounded-lg bg-muted p-[3px] text-muted-foreground">
        {tabs.map((tab) => (
          <NavLink
            key={tab.to}
            to={tab.to}
            end={tab.end}
            className={({ isActive }) => cn(
              'inline-flex h-[calc(100%-1px)] items-center justify-center rounded-md px-2.5 py-0.5 text-sm font-medium whitespace-nowrap transition-all',
              isActive ? 'bg-background text-foreground shadow-sm dark:border dark:border-input dark:bg-input/30' : 'text-foreground/60 hover:text-foreground dark:text-muted-foreground dark:hover:text-foreground',
            )}
          >
            {tab.label}
          </NavLink>
        ))}
      </div>
    </nav>
  )
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
  if (!['overview', 'instances', 'mods'].includes(activeTab) || (nestedPath.length > 0 && activeTab !== 'mods') || (activeTab === 'mods' && !game.capabilities.mods)) {
    return <Navigate replace to={`/games/${game.id}`} />
  }

  return (
    <div className="flex flex-col gap-6">
      <GameHeader game={game} />
      <GameTabs game={game} />
      {activeTab === 'overview' && <InstallStatusCard game={game} />}
      {activeTab === 'instances' && <GameInstances game={game} />}
      {activeTab === 'mods' && game.id === 'valheim' && <ValheimModsTab path={nestedPath} />}
    </div>
  )
}
