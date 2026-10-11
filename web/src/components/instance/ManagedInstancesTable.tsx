import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router-dom'
import { toast } from 'sonner'
import { ManageInstanceDialog } from '@/components/instance/ManageInstanceDialog'
import { GameIcon } from '@/components/GameIcon'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { api } from '@/lib/api-client'
import { useManagedInstanceAction, useManagedInstanceTransition } from '@/lib/queries'
import type { InstanceResources, ManagedInstanceView } from '@/lib/types'
import { formatBytes } from '@/lib/utils'

type ManagedInstancesTableProps = {
  instances: ManagedInstanceView[]
  isLoading?: boolean
  error?: unknown
  emptyMessage: string
  withGame?: boolean
  withButtons?: boolean
  withPort?: boolean
  selectable?: boolean
  selectedIds?: Set<string>
  onToggle?: (id: string) => void
  onToggleAll?: (checked: boolean) => void
}

export function ManagedInstancesTable({
  instances,
  isLoading = false,
  error,
  emptyMessage,
  withGame = true,
  withButtons = true,
  withPort = true,
  selectable = false,
  selectedIds = new Set(),
  onToggle,
  onToggleAll,
}: ManagedInstancesTableProps) {
  const columnCount = Number(selectable) + 2 + Number(withGame) + Number(withPort) + (withButtons ? 2 : 0)

  return (
    <Table>
      <TableHeader>
        <TableRow>
          {selectable && (
            <TableHead>
              <Checkbox
                aria-label="Select visible instances"
                checked={instances.length > 0 && instances.every((instance) => selectedIds.has(instance.id))}
                onCheckedChange={(checked) => onToggleAll?.(Boolean(checked))}
              />
            </TableHead>
          )}
          <TableHead>Name</TableHead>
          {withGame && <TableHead>Game</TableHead>}
          <TableHead>Status</TableHead>
          {withPort && <TableHead className="hidden sm:table-cell">Port</TableHead>}
          {withButtons && <TableHead className="hidden lg:table-cell">CPU / RAM</TableHead>}
          {withButtons && <TableHead className="text-right">Actions</TableHead>}
        </TableRow>
      </TableHeader>
      <TableBody>
        {isLoading && <LoadingRows columnCount={columnCount} />}
        {Boolean(error) && <TableRow><TableCell colSpan={columnCount}><QueryError error={error} /></TableCell></TableRow>}
        {!isLoading && !error && instances.length === 0 && (
          <TableRow><TableCell colSpan={columnCount} className="text-center text-muted-foreground">{emptyMessage}</TableCell></TableRow>
        )}
        {!isLoading && !error && instances.map((instance) => (
          <ManagedInstanceRow
            key={instance.id}
            instance={instance}
            withGame={withGame}
            withButtons={withButtons}
            withPort={withPort}
            selectable={selectable}
            selected={selectedIds.has(instance.id)}
            onToggle={onToggle}
          />
        ))}
      </TableBody>
    </Table>
  )
}

function LoadingRows({ columnCount }: { columnCount: number }) {
  return Array.from({ length: 3 }, (_, index) => (
    <TableRow key={index}><TableCell colSpan={columnCount}><Skeleton className="h-5 w-full" /></TableCell></TableRow>
  ))
}

function ManagedInstanceRow({
  instance,
  withGame,
  withButtons,
  withPort,
  selectable,
  selected,
  onToggle,
}: {
  instance: ManagedInstanceView
  withGame: boolean
  withButtons: boolean
  withPort: boolean
  selectable: boolean
  selected: boolean
  onToggle?: (id: string) => void
}) {
  const resources = useQuery({
    queryKey: ['managed-instances', instance.id, 'resources'],
    queryFn: () => api.get<InstanceResources>(`/instances/${instance.id}/resources`),
    enabled: withButtons && instance.running,
    refetchInterval: 10_000,
  })
  const start = useManagedInstanceAction('start')
  const stop = useManagedInstanceAction('stop')
  const restart = useManagedInstanceAction('restart')
  const transition = useManagedInstanceTransition(instance.id, instance.game, instance.name)
  const busy = start.isPending || stop.isPending || restart.isPending || transition.data !== null
  const port = typeof instance.config.port === 'number' ? instance.config.port : '—'
  const action = { id: instance.id }

  return (
    <TableRow>
      {selectable && (
        <TableCell>
          <Checkbox
            aria-label={`Select ${instance.game} / ${instance.name}`}
            checked={selected}
            onCheckedChange={() => onToggle?.(instance.id)}
          />
        </TableCell>
      )}
      <TableCell className="font-medium">
        <Link className="hover:underline" to={`/instance/${instance.id}`}>{instance.name}</Link>
        <div className="text-xs font-normal text-muted-foreground">Odin {instance.odin_version ? `v${instance.odin_version}` : '—'}</div>
        <div className="flex flex-wrap gap-1">{instance.tags.map((tag) => <Badge key={tag} variant="outline">{tag}</Badge>)}</div>
      </TableCell>
      {withGame && <TableCell><Badge variant="secondary"><GameIcon game={instance.game} className="size-4 rounded-sm" />{instance.game}</Badge></TableCell>}
      <TableCell><Badge variant={instance.running ? 'default' : 'secondary'}>{instance.running ? 'running' : 'stopped'}</Badge></TableCell>
      {withPort && <TableCell className="hidden sm:table-cell">{port}</TableCell>}
      {withButtons && <TableCell className="hidden lg:table-cell">{instance.running && resources.data ? `${resources.data.cpu_percent.toFixed(0)}% · ${formatBytes(resources.data.memory_bytes)}` : '—'}</TableCell>}
      {withButtons && (
        <TableCell className="text-right">
          <div className="flex flex-wrap justify-end gap-2">
            <ManageInstanceDialog instance={instance} />
            {instance.running ? (
              <div className="flex justify-end gap-2">
                <Button size="sm" variant="outline" disabled={busy} onClick={() => restart.mutate(action, { onError: (error) => toast.error(error.message) })}>Restart</Button>
                <Button size="sm" variant="outline" disabled={busy} onClick={() => stop.mutate(action, { onError: (error) => toast.error(error.message) })}>Stop</Button>
              </div>
            ) : <Button size="sm" disabled={busy} onClick={() => start.mutate(action, { onError: (error) => toast.error(error.message) })}>Start</Button>}
          </div>
        </TableCell>
      )}
    </TableRow>
  )
}
