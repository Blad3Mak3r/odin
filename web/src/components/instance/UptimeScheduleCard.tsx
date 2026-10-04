import { useState } from 'react'
import { toast } from 'sonner'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { useSetUptimeSchedule, useUptimeSchedule } from '@/lib/queries'
import type { GameId, UptimeScheduleView } from '@/lib/types'

export function UptimeScheduleCard({ game, name }: { game: GameId; name: string }) {
  const schedule = useUptimeSchedule(game, name)
  if (schedule.isError) return <QueryError error={schedule.error} />
  if (!schedule.data) return <Skeleton className="h-48 w-full" />
  return <UptimeScheduleForm key={JSON.stringify(schedule.data)} game={game} name={name} initial={schedule.data} />
}

function UptimeScheduleForm({ game, name, initial }: { game: GameId; name: string; initial: UptimeScheduleView }) {
  const setSchedule = useSetUptimeSchedule(game, name)
  const [enabled, setEnabled] = useState(initial.enabled)
  const [startTime, setStartTime] = useState(initial.start_time)
  const [stopTime, setStopTime] = useState(initial.stop_time)
  const timesMatch = startTime === stopTime

  const save = () => {
    if (timesMatch) return
    setSchedule.mutate(
      { enabled, start_time: startTime, stop_time: stopTime },
      {
        onSuccess: () => toast.success(enabled ? 'Operating hours saved' : 'Operating hours disabled'),
        onError: (error) => toast.error(error.message),
      },
    )
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Operating hours</CardTitle>
        <CardDescription>
          Start and stop this server automatically each day using the host system's local time zone.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex items-center justify-between rounded-xl border p-3">
          <div>
            <Label htmlFor="uptime-schedule-enabled">Automatic start and stop</Label>
            <p className="text-xs text-muted-foreground">Outside this window, a running server is stopped automatically.</p>
          </div>
          <Switch id="uptime-schedule-enabled" checked={enabled} onCheckedChange={setEnabled} />
        </div>
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="flex flex-col gap-2">
            <Label htmlFor="uptime-schedule-start">Start time</Label>
            <Input id="uptime-schedule-start" type="time" value={startTime} onChange={(event) => setStartTime(event.target.value)} />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="uptime-schedule-stop">Stop time</Label>
            <Input id="uptime-schedule-stop" type="time" value={stopTime} onChange={(event) => setStopTime(event.target.value)} />
          </div>
        </div>
        {timesMatch && <p className="text-xs text-destructive">Start and stop times must be different.</p>}
        <p className="text-xs text-muted-foreground">A stop time earlier than the start time runs overnight, for example 22:00 to 06:00.</p>
        <Button className="w-fit" onClick={save} disabled={timesMatch || setSchedule.isPending}>Save operating hours</Button>
      </CardContent>
    </Card>
  )
}
