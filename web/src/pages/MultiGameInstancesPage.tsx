import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api-client'
import { CreateManagedInstanceDialog } from '@/components/instance/CreateManagedInstanceDialog'
import { ManagedInstancesTable } from '@/components/instance/ManagedInstancesTable'
import { GameIcon } from '@/components/GameIcon'
import { useState } from 'react'
import { toast } from 'sonner'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { QueryError } from '@/components/QueryError'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { useGames, useIsAdmin, useManagedInstances } from '@/lib/queries'
import type { GameId } from '@/lib/types'

type Filter = 'all' | GameId
type BulkResult = { id: string; game: GameId | null; name: string | null; ok: boolean; error: string | null; job_id: string | null }

export function MultiGameInstancesPage() {
  const isAdmin = useIsAdmin()
  const instances = useManagedInstances()
  const games = useGames()
  const [filter, setFilter] = useState<Filter>('all')
  const [search, setSearch] = useState('')
  const [sort, setSort] = useState('name')
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const client = useQueryClient()
  const visible = (instances.data ?? []).filter((instance) => (filter === 'all' || instance.game === filter) && [instance.name, ...instance.tags].join(' ').toLowerCase().includes(search.toLowerCase())).sort((a, b) => sort === 'game' ? a.game.localeCompare(b.game) || a.name.localeCompare(b.name) : sort === 'status' ? Number(b.running) - Number(a.running) || a.name.localeCompare(b.name) : a.name.localeCompare(b.name))
  const targets = (instances.data ?? []).filter((instance) => selected.has(instance.id))
  const [results, setResults] = useState<BulkResult[]>([])
  const bulk = useMutation({
    mutationFn: (operation: string) => api.post<BulkResult[]>(`/instances/bulk/games/${operation}`, { ids: targets.map(({ id }) => id) }),
    onSuccess: (data) => { setResults(data); setSelected(new Set()); client.invalidateQueries({ queryKey: ['managed-instances'] }); client.invalidateQueries({ queryKey: ['jobs'] }) },
    onError: (error) => toast.error(error.message),
  })
  const toggle = (id: string) => setSelected((previous) => { const next = new Set(previous); if (next.has(id)) next.delete(id); else next.add(id); return next })

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Instances"
        description="Every game server Odin manages."
        action={isAdmin ? <CreateManagedInstanceDialog games={games.data ?? []} /> : undefined}
      />
      <ToggleGroup value={[filter]} onValueChange={(value) => value[0] && setFilter(value[0] as Filter)} variant="outline" size="sm">
        <ToggleGroupItem value="all">All</ToggleGroupItem>
        {games.data?.map((game) => <ToggleGroupItem key={game.id} value={game.id}><GameIcon game={game.id} />{game.name}</ToggleGroupItem>)}
      </ToggleGroup>
      {games.isError && <QueryError error={games.error} />}
      <div className="flex flex-wrap gap-3">
        <Input aria-label="Search instances or tags" placeholder="Search instances or tags…" value={search} onChange={(e) => setSearch(e.target.value)} className="max-w-sm" />
        <select aria-label="Sort instances" className="rounded-md border bg-background px-3 text-sm" value={sort} onChange={(e) => setSort(e.target.value)}><option value="name">Name</option><option value="game">Game</option><option value="status">Running first</option></select>
      </div>
      {targets.length > 0 && <div className="flex flex-wrap items-center gap-2 rounded-xl border p-3">
        <span className="text-sm">{targets.length} selected</span>
        {['start', 'stop', 'restart', ...(targets.every((target) => target.capabilities.mods) ? ['mods', 'bepinex'] : [])].map((operation) => <Button key={operation} size="sm" variant="outline" disabled={bulk.isPending || targets.length > 100} onClick={() => bulk.mutate(operation)}>{operation === 'mods' ? 'Update mods' : operation === 'bepinex' ? 'Update BepInEx' : operation}</Button>)}
        <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>Clear selection</Button>
      </div>}
      {results.length > 0 && <div className="text-sm" role="status">{results.map((result) => <p key={result.id} className={result.ok ? 'text-muted-foreground' : 'text-destructive'}>{result.game ?? 'unknown'} / {result.name ?? result.id}: {result.ok ? 'Operation accepted' : result.error}</p>)}</div>}
      <ManagedInstancesTable
        instances={visible}
        isLoading={instances.isLoading}
        error={instances.isError ? instances.error : undefined}
        emptyMessage="No instances for this filter."
        selectable={isAdmin}
        selectedIds={selected}
        onToggle={toggle}
        onToggleAll={(checked) => setSelected((previous) => {
          const next = new Set(previous)
          for (const instance of visible) {
            if (checked) next.add(instance.id)
            else next.delete(instance.id)
          }
          return next
        })}
      />
    </div>
  )
}
