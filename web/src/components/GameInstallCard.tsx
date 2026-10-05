import { Loader2 } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { toast } from 'sonner'
import { GameIcon } from '@/components/GameIcon'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useGameInstallStatus, useInstallGame, useJobs } from '@/lib/queries'
import type { GameView } from '@/lib/types'

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
