import { Loader2 } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { JobProgress } from '@/components/JobProgress'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { useJobSocket } from '@/hooks/useJobSocket'
import { useBackups, useCreateBackup, useDeleteBackup, useRestoreBackup } from '@/lib/queries'
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
              <TableHead>Created</TableHead>
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
                      variant="ghost"
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
