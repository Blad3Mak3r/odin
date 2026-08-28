import { Loader2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { JobProgress } from '@/components/JobProgress'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { useJobSocket } from '@/hooks/useJobSocket'
import {
  useBackups,
  useBackupSchedule,
  useCreateBackup,
  useDeleteBackup,
  useRestoreBackup,
  useSetBackupSchedule,
} from '@/lib/queries'
import { formatBytes, formatRelativeTime } from '@/lib/utils'

export function BackupsTab({ name, running }: { name: string; running: boolean }) {
  const backups = useBackups(name)
  const createBackup = useCreateBackup()
  const restoreBackup = useRestoreBackup()
  const deleteBackup = useDeleteBackup()
  const [jobId, setJobId] = useState<string | null>(null)
  const job = useJobSocket(jobId)
  const { confirm, dialog } = useConfirmDialog()

  const handleRestore = async (backupId: string) => {
    const confirmed = await confirm({
      title: `Restore backup '${backupId}'?`,
      description: `Restore '${name}' from this backup? The current saves are backed up first, so this isn't a one-way action, but the instance's world will be overwritten.`,
      confirmLabel: 'Restore',
    })
    if (!confirmed) return
    restoreBackup.mutate(
      { name, backupId },
      {
        onSuccess: (handle) => setJobId(handle.id),
        onError: (e) => toast.error(e.message),
      },
    )
  }

  const handleDelete = async (backupId: string) => {
    const confirmed = await confirm({
      title: `Delete backup '${backupId}'?`,
      description: 'Permanently delete this backup file. This cannot be undone.',
      confirmLabel: 'Delete',
    })
    if (!confirmed) return
    deleteBackup.mutate(
      { name, backupId },
      { onError: (e) => toast.error(e.message) },
    )
  }

  if (backups.isError) {
    return <QueryError error={backups.error} />
  }

  return (
    <div className="flex flex-col gap-3">
      {dialog}
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Backups</h2>
        <Button
          size="sm"
          variant="outline"
          disabled={createBackup.isPending}
          onClick={() =>
            createBackup.mutate(name, {
              onSuccess: (handle) => setJobId(handle.id),
              onError: (e) => toast.error(e.message),
            })
          }
        >
          {createBackup.isPending && <Loader2 className="size-4 animate-spin" />}
          Create backup
        </Button>
      </div>

      <BackupScheduleSection name={name} />

      {running && (
        <p className="text-xs text-muted-foreground">
          Restore is disabled while '{name}' is running — stop it first.
        </p>
      )}

      {backups.isLoading && (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
        </div>
      )}

      {backups.data?.length === 0 && (
        <p className="text-sm text-muted-foreground">No backups yet.</p>
      )}

      {backups.data && backups.data.length > 0 && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-full">Created</TableHead>
              <TableHead className="hidden sm:table-cell">Size</TableHead>
              <TableHead className="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {backups.data.map((backup) => (
              <TableRow key={backup.id}>
                <TableCell>{formatRelativeTime(backup.created_at)}</TableCell>
                <TableCell className="hidden text-muted-foreground sm:table-cell">
                  {formatBytes(backup.size_bytes)}
                </TableCell>
                <TableCell className="text-right">
                  <div className="flex justify-end gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={running || restoreBackup.isPending}
                      onClick={() => handleRestore(backup.id)}
                    >
                      Restore
                    </Button>
                    <Button
                      size="sm"
                      variant="destructive"
                      disabled={deleteBackup.isPending}
                      onClick={() => handleDelete(backup.id)}
                    >
                      Delete
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      {jobId && <JobProgress log={job.log} status={job.status} connected={job.connected} />}
    </div>
  )
}

function BackupScheduleSection({ name }: { name: string }) {
  const schedule = useBackupSchedule(name)
  const setSchedule = useSetBackupSchedule(name)

  const [enabled, setEnabled] = useState(false)
  const [intervalHours, setIntervalHours] = useState('24')
  const [retainCount, setRetainCount] = useState('7')

  useEffect(() => {
    if (!schedule.data) return
    setEnabled(schedule.data.enabled)
    setIntervalHours(String(schedule.data.interval_hours))
    setRetainCount(String(schedule.data.retain_count))
  }, [schedule.data])

  if (schedule.isLoading) {
    return <Skeleton className="h-24 w-full" />
  }
  if (schedule.isError) {
    return <QueryError error={schedule.error} />
  }

  const intervalValue = Number(intervalHours)
  const retainValue = Number(retainCount)
  const invalid =
    intervalHours.trim() === '' ||
    retainCount.trim() === '' ||
    Number.isNaN(intervalValue) ||
    Number.isNaN(retainValue) ||
    intervalValue < 1 ||
    retainValue < 1

  const handleSave = () => {
    if (invalid) return
    setSchedule.mutate(
      { interval_hours: intervalValue, retain_count: retainValue, enabled },
      {
        onSuccess: () => toast.success('Backup schedule saved'),
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <div className="flex flex-col gap-3 rounded-xl border p-3">
      <div className="flex items-center justify-between">
        <div>
          <p className="text-sm font-medium">Automatic backups</p>
          <p className="text-xs text-muted-foreground">
            {schedule.data?.last_run_at
              ? `Last ran ${formatRelativeTime(schedule.data.last_run_at)}`
              : 'Not run yet.'}
          </p>
        </div>
        <Switch checked={enabled} onCheckedChange={setEnabled} />
      </div>
      <div className="flex flex-wrap items-end gap-3">
        <div className="flex flex-col gap-1">
          <Label htmlFor="backup-interval">Every (hours)</Label>
          <Input
            id="backup-interval"
            type="number"
            min={1}
            className="w-24"
            value={intervalHours}
            onChange={(e) => setIntervalHours(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="backup-retain">Keep last</Label>
          <Input
            id="backup-retain"
            type="number"
            min={1}
            className="w-24"
            value={retainCount}
            onChange={(e) => setRetainCount(e.target.value)}
          />
        </div>
        <Button size="sm" disabled={invalid || setSchedule.isPending} onClick={handleSave}>
          {setSchedule.isPending && <Loader2 className="size-4 animate-spin" />}
          Save
        </Button>
      </div>
    </div>
  )
}
