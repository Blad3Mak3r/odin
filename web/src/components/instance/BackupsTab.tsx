import { CloudUpload } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { JobProgress } from '@/components/JobProgress'
import { QueryError } from '@/components/QueryError'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'
import { Switch } from '@/components/ui/switch'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { useJobSocket } from '@/hooks/useJobSocket'
import {
  useBackups,
  useBackupSchedule,
  useBackupStorage,
  useCreateBackup,
  useDeleteBackup,
  useRestoreBackup,
  useSetBackupSchedule,
  useSetBackupStorage,
} from '@/lib/queries'
import type { BackupScheduleView, BackupStorageProvider, BackupStorageView } from '@/lib/types'
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
        onError: (error) => toast.error(error.message),
      },
    )
  }

  const handleDelete = async (backupId: string) => {
    const confirmed = await confirm({
      title: `Delete backup '${backupId}'?`,
      description: 'Permanently delete this backup from its storage location. This cannot be undone.',
      confirmLabel: 'Delete',
    })
    if (!confirmed) return
    deleteBackup.mutate(
      { name, backupId },
      { onError: (error) => toast.error(error.message) },
    )
  }

  if (backups.isError) return <QueryError error={backups.error} />

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
              onError: (error) => toast.error(error.message),
            })
          }
        >
          {createBackup.isPending && <Spinner data-icon="inline-start" />}
          Create backup
        </Button>
      </div>

      <BackupScheduleSection name={name} />
      <RemoteBackupStorageSection name={name} />

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
              <TableHead className="hidden md:table-cell">Storage</TableHead>
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
                <TableCell className="hidden md:table-cell">
                  <Badge variant={backup.storage === 'local' ? 'outline' : 'secondary'}>
                    {backupStorageLabel(backup.storage)}
                  </Badge>
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

  if (schedule.isLoading) return <Skeleton className="h-24 w-full" />
  if (schedule.isError) return <QueryError error={schedule.error} />
  if (!schedule.data) return null

  const key = `${schedule.data.enabled}-${schedule.data.interval_hours}-${schedule.data.retain_count}-${schedule.data.last_run_at}`
  return <BackupScheduleForm key={key} name={name} schedule={schedule.data} />
}

function BackupScheduleForm({ name, schedule }: { name: string; schedule: BackupScheduleView }) {
  const setSchedule = useSetBackupSchedule(name)
  const [enabled, setEnabled] = useState(schedule.enabled)
  const [intervalHours, setIntervalHours] = useState(String(schedule.interval_hours))
  const [retainCount, setRetainCount] = useState(String(schedule.retain_count))

  const intervalValue = Number(intervalHours)
  const retainValue = Number(retainCount)
  const invalid =
    intervalHours.trim() === '' ||
    retainCount.trim() === '' ||
    Number.isNaN(intervalValue) ||
    Number.isNaN(retainValue) ||
    intervalValue < 1 ||
    retainValue < 1

  return (
    <form
      className="flex flex-col gap-3 rounded-xl border p-3"
      onSubmit={(event) => {
        event.preventDefault()
        if (invalid) return
        setSchedule.mutate(
          { interval_hours: intervalValue, retain_count: retainValue, enabled },
          {
            onSuccess: () => toast.success('Backup schedule saved'),
            onError: (error) => toast.error(error.message),
          },
        )
      }}
    >
      <Field orientation="horizontal">
        <FieldContent>
          <FieldLabel htmlFor="backup-schedule-enabled">Automatic backups</FieldLabel>
          <FieldDescription>
            {schedule.last_run_at
              ? `Last ran ${formatRelativeTime(schedule.last_run_at)}`
              : 'Not run yet.'}
          </FieldDescription>
        </FieldContent>
        <Switch
          id="backup-schedule-enabled"
          checked={enabled}
          onCheckedChange={setEnabled}
          aria-label="Enable automatic backups"
        />
      </Field>
      <FieldGroup className="flex-row flex-wrap items-end gap-3">
        <Field className="w-24">
          <FieldLabel htmlFor="backup-interval">Every (hours)</FieldLabel>
          <Input
            id="backup-interval"
            type="number"
            min={1}
            value={intervalHours}
            onChange={(event) => setIntervalHours(event.target.value)}
          />
        </Field>
        <Field className="w-24">
          <FieldLabel htmlFor="backup-retain">Keep last</FieldLabel>
          <Input
            id="backup-retain"
            type="number"
            min={1}
            value={retainCount}
            onChange={(event) => setRetainCount(event.target.value)}
          />
        </Field>
        <Button size="sm" type="submit" disabled={invalid || setSchedule.isPending}>
          {setSchedule.isPending && <Spinner data-icon="inline-start" />}
          Save schedule
        </Button>
      </FieldGroup>
    </form>
  )
}

function RemoteBackupStorageSection({ name }: { name: string }) {
  const storage = useBackupStorage(name)

  if (storage.isLoading) return <Skeleton className="h-80 w-full" />
  if (storage.isError) return <QueryError error={storage.error} />
  if (!storage.data) return null

  const key = `${storage.data.enabled}-${storage.data.provider}-${storage.data.endpoint}-${storage.data.region}-${storage.data.bucket}-${storage.data.prefix}-${storage.data.access_key_id}-${storage.data.secret_access_key_configured}`
  return <RemoteBackupStorageForm key={key} name={name} storage={storage.data} />
}

function RemoteBackupStorageForm({ name, storage }: { name: string; storage: BackupStorageView }) {
  const setStorage = useSetBackupStorage(name)
  const [enabled, setEnabled] = useState(storage.enabled)
  const [provider, setProvider] = useState<BackupStorageProvider>(storage.provider ?? 'aws_s3')
  const [endpoint, setEndpoint] = useState(
    storage.provider === 'cloudflare_r2' ? storage.endpoint : '',
  )
  const [region, setRegion] = useState(storage.region || 'us-east-1')
  const [bucket, setBucket] = useState(storage.bucket)
  const [prefix, setPrefix] = useState(storage.prefix)
  const [accessKeyId, setAccessKeyId] = useState(storage.access_key_id)
  const [secretAccessKey, setSecretAccessKey] = useState('')

  const secretRequired =
    !storage.secret_access_key_configured || accessKeyId !== storage.access_key_id
  const invalid =
    bucket.trim() === '' ||
    accessKeyId.trim() === '' ||
    (provider === 'aws_s3' && region.trim() === '') ||
    (provider === 'cloudflare_r2' && endpoint.trim() === '') ||
    (secretRequired && secretAccessKey.trim() === '')

  return (
    <form
      className="flex flex-col gap-4 rounded-xl border p-3"
      onSubmit={(event) => {
        event.preventDefault()
        if (invalid) return
        setStorage.mutate(
          {
            enabled,
            provider,
            endpoint: provider === 'cloudflare_r2' ? endpoint : null,
            region: provider === 'aws_s3' ? region : null,
            bucket,
            prefix,
            access_key_id: accessKeyId,
            secret_access_key: secretAccessKey.trim() || null,
          },
          {
            onSuccess: () => toast.success('Remote backup storage saved'),
            onError: (error) => toast.error(error.message),
          },
        )
      }}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="text-sm font-medium">Remote backup storage</p>
          <p className="text-xs text-muted-foreground">Upload new backups to an S3-compatible bucket.</p>
        </div>
        <Badge variant={storage.configured ? 'secondary' : 'outline'}>
          {storage.configured ? (enabled ? 'enabled' : 'disabled') : 'not configured'}
        </Badge>
      </div>

      <Field orientation="horizontal">
        <FieldContent>
          <FieldLabel htmlFor="backup-storage-enabled">Use remote storage</FieldLabel>
          <FieldDescription>Applies to manual, scheduled, and pre-restore backups.</FieldDescription>
        </FieldContent>
        <Switch
          id="backup-storage-enabled"
          checked={enabled}
          onCheckedChange={setEnabled}
          aria-label="Use remote backup storage"
        />
      </Field>

      <FieldSet>
        <FieldLegend variant="label">Provider</FieldLegend>
        <ToggleGroup
          value={[provider]}
          onValueChange={(value) => value[0] && setProvider(value[0] as BackupStorageProvider)}
          variant="outline"
          spacing={0}
          aria-label="Remote backup provider"
        >
          <ToggleGroupItem type="button" value="aws_s3">AWS S3</ToggleGroupItem>
          <ToggleGroupItem type="button" value="cloudflare_r2">Cloudflare R2</ToggleGroupItem>
        </ToggleGroup>
      </FieldSet>

      <FieldGroup className="grid gap-3 sm:grid-cols-2">
        {provider === 'cloudflare_r2' ? (
          <Field className="sm:col-span-2">
            <FieldLabel htmlFor="backup-storage-endpoint">R2 endpoint</FieldLabel>
            <Input
              id="backup-storage-endpoint"
              type="url"
              placeholder="https://account-id.r2.cloudflarestorage.com"
              value={endpoint}
              onChange={(event) => setEndpoint(event.target.value)}
              required
            />
            <FieldDescription>Use the S3 API endpoint shown in the Cloudflare R2 dashboard.</FieldDescription>
          </Field>
        ) : (
          <Field>
            <FieldLabel htmlFor="backup-storage-region">AWS region</FieldLabel>
            <Input
              id="backup-storage-region"
              placeholder="us-east-1"
              value={region}
              onChange={(event) => setRegion(event.target.value)}
              required
            />
          </Field>
        )}
        <Field>
          <FieldLabel htmlFor="backup-storage-bucket">Bucket</FieldLabel>
          <Input
            id="backup-storage-bucket"
            placeholder="odin-backups"
            value={bucket}
            onChange={(event) => setBucket(event.target.value)}
            required
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="backup-storage-prefix">Key prefix</FieldLabel>
          <Input
            id="backup-storage-prefix"
            placeholder="odin"
            value={prefix}
            onChange={(event) => setPrefix(event.target.value)}
            maxLength={200}
          />
          <FieldDescription>Optional folder prefix inside the bucket.</FieldDescription>
        </Field>
        <Field>
          <FieldLabel htmlFor="backup-storage-access-key">Access key ID</FieldLabel>
          <Input
            id="backup-storage-access-key"
            autoComplete="off"
            value={accessKeyId}
            onChange={(event) => setAccessKeyId(event.target.value)}
            required
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="backup-storage-secret-key">Secret access key</FieldLabel>
          <Input
            id="backup-storage-secret-key"
            type="password"
            autoComplete="new-password"
            placeholder={storage.secret_access_key_configured ? 'Leave blank to keep current secret' : ''}
            value={secretAccessKey}
            onChange={(event) => setSecretAccessKey(event.target.value)}
            required={secretRequired}
          />
          <FieldDescription>The secret is stored on the Odin host and is never returned by the API.</FieldDescription>
        </Field>
      </FieldGroup>

      <Alert>
        <CloudUpload />
        <AlertTitle>Local file lifecycle</AlertTitle>
        <AlertDescription>
          After a confirmed upload, Odin removes the local ZIP. If the upload fails, the ZIP stays
          local and the job reports the error so no backup is lost.
        </AlertDescription>
      </Alert>

      <div>
        <Button type="submit" size="sm" disabled={invalid || setStorage.isPending}>
          {setStorage.isPending && <Spinner data-icon="inline-start" />}
          Save remote storage
        </Button>
      </div>
    </form>
  )
}

function backupStorageLabel(storage: 'local' | BackupStorageProvider) {
  switch (storage) {
    case 'aws_s3':
      return 'AWS S3'
    case 'cloudflare_r2':
      return 'Cloudflare R2'
    case 'local':
      return 'Local'
  }
}
