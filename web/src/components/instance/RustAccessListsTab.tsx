import { X } from 'lucide-react'
import { useState } from 'react'
import { Navigate, useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { QueryError } from '@/components/QueryError'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useAddRustAccessListEntry, useRemoveRustAccessListEntry, useRustAccessList } from '@/lib/queries'
import type { RustAccessListKind } from '@/lib/types'

const LIST_TABS = ['owner', 'moderator', 'banned'] as const
type RustListTab = (typeof LIST_TABS)[number]

function isRustListTab(value: string | undefined): value is RustListTab {
  return LIST_TABS.some((tab) => tab === value)
}

export function RustAccessListsTab({ id, path, running, basePath }: { id: string; path: string[]; running: boolean; basePath: string }) {
  const navigate = useNavigate()
  const [tab, ...rest] = path

  if (!isRustListTab(tab) || rest.length > 0) return <Navigate to={`${basePath}/owner`} replace />

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        Rust loads these access lists when the server starts. Stop it before making changes, then start it again to apply them.
      </p>
      <Tabs value={tab} onValueChange={(value) => navigate(`${basePath}/${value}`)}>
        <TabsList variant="line">
          <TabsTrigger value="owner">Owners</TabsTrigger>
          <TabsTrigger value="moderator">Moderators</TabsTrigger>
          <TabsTrigger value="banned">Banned</TabsTrigger>
        </TabsList>
        {tab === 'owner' && <TabsContent value="owner"><RustSteamIdListEditor id={id} kind="owner" running={running} /></TabsContent>}
        {tab === 'moderator' && <TabsContent value="moderator"><RustSteamIdListEditor id={id} kind="moderator" running={running} /></TabsContent>}
        {tab === 'banned' && <TabsContent value="banned"><RustSteamIdListEditor id={id} kind="banned" running={running} /></TabsContent>}
      </Tabs>
    </div>
  )
}

function RustSteamIdListEditor({ id, kind, running }: { id: string; kind: RustAccessListKind; running: boolean }) {
  const list = useRustAccessList(id, kind)
  const addEntry = useAddRustAccessListEntry(id, kind)
  const removeEntry = useRemoveRustAccessListEntry(id, kind)
  const [newId, setNewId] = useState('')
  const { confirm, dialog } = useConfirmDialog()

  if (list.isError) return <QueryError error={list.error} />
  if (list.isLoading || !list.data) {
    return <div className="grid gap-1 sm:grid-cols-2 lg:grid-cols-3"><Skeleton className="h-10 w-full" /><Skeleton className="h-10 w-full" /></div>
  }

  const ids = list.data.ids
  const addId = () => {
    const id = newId.trim()
    if (running || !id || ids.includes(id)) return
    addEntry.mutate(id, { onSuccess: () => setNewId(''), onError: (error) => toast.error(error.message) })
  }
  const removeId = async (id: string) => {
    if (running) return
    const confirmed = await confirm({ title: 'Remove entry?', description: `Remove '${id}' from this list?`, confirmLabel: 'Remove' })
    if (confirmed) removeEntry.mutate(id, { onError: (error) => toast.error(error.message) })
  }

  return (
    <div className="flex flex-col gap-3">
      {dialog}
      {ids.length === 0 && <p className="text-sm text-muted-foreground">No entries.</p>}
      <div className="grid gap-1 sm:grid-cols-2 lg:grid-cols-3">
        {ids.map((id) => (
          <div key={id} className="flex items-center justify-between rounded-xl border px-3 py-2">
            <span className="font-mono text-sm">{id}</span>
            <Button size="icon" variant="ghost" aria-label={`Remove ${id}`} disabled={running || removeEntry.isPending} onClick={() => removeId(id)}><X className="size-4" /></Button>
          </div>
        ))}
      </div>
      <div className="flex max-w-sm gap-2">
        <Input placeholder="17-digit SteamID64…" value={newId} disabled={running} onChange={(event) => setNewId(event.target.value)} onKeyDown={(event) => event.key === 'Enter' && addId()} />
        <Button onClick={addId} disabled={running || !newId.trim() || addEntry.isPending}>Add</Button>
      </div>
    </div>
  )
}
