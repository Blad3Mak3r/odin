import { Loader2 } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { Link, Navigate, useNavigate, useParams } from 'react-router-dom'
import { toast } from 'sonner'
import palworldBanner from '@/assets/game-banners/palworld.webp'
import runescapeDragonwildsBanner from '@/assets/game-banners/runescape-dragonwilds.webp'
import rustBanner from '@/assets/game-banners/rust.webp'
import valheimBanner from '@/assets/game-banners/valheim.webp'
import vrisingBanner from '@/assets/game-banners/vrising.webp'
import { GameIcon } from '@/components/GameIcon'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { ValheimModsTab } from '@/pages/GlobalModsPage'
import { useGameInstallStatus, useGames, useInstallGame, useJobs, useManagedInstances } from '@/lib/queries'
import type { GameId, GameView } from '@/lib/types'

const GAME_DESCRIPTIONS: Record<GameId, string> = {
  valheim: 'Build, explore, and survive with a dedicated Valheim server.',
  rust: 'Host and manage your Rust dedicated servers.',
  vrising: 'Run your V Rising server and manage its world.',
  palworld: 'Operate your Palworld dedicated server from one place.',
  'runescape-dragonwilds': 'Host your RuneScape: Dragonwilds server.',
}

const GAME_BANNERS: Record<GameId, string> = {
  valheim: valheimBanner,
  rust: rustBanner,
  vrising: vrisingBanner,
  palworld: palworldBanner,
  'runescape-dragonwilds': runescapeDragonwildsBanner,
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

  if (instances.isError) return <QueryError error={instances.error} />
  if (instances.isLoading) return <Skeleton className="h-28" />
  if (gameInstances.length === 0) {
    return <p className="text-sm text-muted-foreground">No {game.name} instances yet.</p>
  }

  return (
    <div className="grid gap-3 md:grid-cols-2">
      {gameInstances.map((instance) => (
        <Card key={instance.id}>
          <CardHeader className="flex-row items-center justify-between space-y-0">
            <div>
              <CardTitle className="text-base">{instance.name}</CardTitle>
              <CardDescription>Created {new Date(instance.created_at).toLocaleDateString()}</CardDescription>
            </div>
            <Badge variant={instance.running ? 'default' : 'secondary'}>{instance.running ? 'Running' : 'Stopped'}</Badge>
          </CardHeader>
          <CardContent>
            <Button size="sm" variant="outline" render={<Link to={`/instance/${instance.id}`} />}>Manage instance</Button>
          </CardContent>
        </Card>
      ))}
    </div>
  )
}

export function GameDetailPage() {
  const { game: gameParam, '*': tabPath } = useParams<{ game: string; '*': string }>()
  const navigate = useNavigate()
  const games = useGames()
  const game = games.data?.find((candidate) => candidate.id === gameParam)
  const [tab, ...nestedPath] = tabPath?.split('/').filter(Boolean) ?? []
  const activeTab = tab || 'overview'

  if (games.isError) return <QueryError error={games.error} />
  if (games.isLoading) return <Skeleton className="h-56" />
  if (!game) return <Navigate replace to="/games" />
  if (!['overview', 'instances', 'mods'].includes(activeTab) || (nestedPath.length > 0 && activeTab !== 'mods') || (activeTab === 'mods' && !game.capabilities.mods)) {
    return <Navigate replace to={`/games/${game.id}`} />
  }

  const targetPath = (next: string) => next === 'overview' ? `/games/${game.id}` : `/games/${game.id}/${next}`

  return (
    <div className="flex flex-col gap-6">
      <Tabs value={activeTab} onValueChange={(value) => navigate(targetPath(value))}>
        <div className="overflow-x-auto">
          <TabsList className="w-max">
            <TabsTrigger value="overview">Overview</TabsTrigger>
            <TabsTrigger value="instances">Instances</TabsTrigger>
            {game.capabilities.mods && <TabsTrigger value="mods">Mods</TabsTrigger>}
          </TabsList>
        </div>
        <TabsContent value="overview" className="flex flex-col gap-6">
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
          <InstallStatusCard game={game} />
        </TabsContent>
        <TabsContent value="instances"><GameInstances game={game} /></TabsContent>
        {game.id === 'valheim' && <TabsContent value="mods"><ValheimModsTab path={nestedPath} /></TabsContent>}
      </Tabs>
    </div>
  )
}
