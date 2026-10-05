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
import { useAddVRisingAccessListEntry, useRemoveVRisingAccessListEntry, useVRisingAccessList } from '@/lib/queries'
import type { VRisingAccessListKind } from '@/lib/types'

const LIST_TABS = ['admin', 'banned'] as const
type VRisingListTab = (typeof LIST_TABS)[number]

function isVRisingListTab(value: string | undefined): value is VRisingListTab {
  return LIST_TABS.some((tab) => tab === value)
}

export function VRisingAccessListsTab({ name, path, basePath }: { name: string; path: string[]; basePath: string }) {
  const navigate = useNavigate()
  const [tab, ...rest] = path
  if (!isVRisingListTab(tab) || rest.length > 0) return <Navigate to={`${basePath}/admin`} replace />

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">V Rising reads administrator and ban lists from its isolated persistent-data directory. Changes can be made while the server is running.</p>
      <Tabs value={tab} onValueChange={(value) => navigate(`${basePath}/${value}`)}>
        <TabsList variant="line"><TabsTrigger value="admin">Administrators</TabsTrigger><TabsTrigger value="banned">Banned</TabsTrigger></TabsList>
        <TabsContent value="admin"><SteamIdListEditor name={name} kind="admin" /></TabsContent>
        <TabsContent value="banned"><SteamIdListEditor name={name} kind="banned" /></TabsContent>
      </Tabs>
    </div>
  )
}

function SteamIdListEditor({ name, kind }: { name: string; kind: VRisingAccessListKind }) {
  const list = useVRisingAccessList(name, kind)
  const addEntry = useAddVRisingAccessListEntry(name, kind)
  const removeEntry = useRemoveVRisingAccessListEntry(name, kind)
  const [newId, setNewId] = useState('')
  const { confirm, dialog } = useConfirmDialog()
  if (list.isError) return <QueryError error={list.error} />
  if (list.isLoading || !list.data) return <div className="grid gap-1 sm:grid-cols-2 lg:grid-cols-3"><Skeleton className="h-10 w-full" /><Skeleton className="h-10 w-full" /></div>

  const ids = list.data.ids
  const addId = () => {
    const id = newId.trim()
    if (!id || ids.includes(id)) return
    addEntry.mutate(id, { onSuccess: () => setNewId(''), onError: (error) => toast.error(error.message) })
  }
  const removeId = async (id: string) => {
    if (await confirm({ title: 'Remove entry?', description: `Remove '${id}' from this list?`, confirmLabel: 'Remove' })) {
      removeEntry.mutate(id, { onError: (error) => toast.error(error.message) })
    }
  }
  return <div className="flex flex-col gap-3">
    {dialog}
    {ids.length === 0 && <p className="text-sm text-muted-foreground">No entries.</p>}
    <div className="grid gap-1 sm:grid-cols-2 lg:grid-cols-3">{ids.map((id) => <div key={id} className="flex items-center justify-between rounded-xl border px-3 py-2"><span className="font-mono text-sm">{id}</span><Button size="icon" variant="ghost" aria-label={`Remove ${id}`} disabled={removeEntry.isPending} onClick={() => removeId(id)}><X className="size-4" /></Button></div>)}</div>
    <div className="flex max-w-sm gap-2"><Input placeholder="17-digit SteamID64…" value={newId} onChange={(event) => setNewId(event.target.value)} onKeyDown={(event) => event.key === 'Enter' && addId()} /><Button onClick={addId} disabled={!newId.trim() || addEntry.isPending}>Add</Button></div>
  </div>
}
