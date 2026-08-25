import { X } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { useList, useSetList } from '@/lib/queries'
import type { ListKind } from '@/lib/types'

export function SteamIdListEditor({ name, kind }: { name: string; kind: ListKind }) {
  const list = useList(name, kind)
  const setList = useSetList(name, kind)
  const [newId, setNewId] = useState('')
  const { confirm, dialog } = useConfirmDialog()

  if (list.isError) {
    return <QueryError error={list.error} />
  }

  if (list.isLoading || !list.data) {
    return <p className="text-sm text-muted-foreground">Loading…</p>
  }

  const ids = list.data.ids

  const addId = () => {
    const id = newId.trim()
    if (!id || ids.includes(id)) return
    setList.mutate([...ids, id], {
      onSuccess: () => setNewId(''),
      onError: (e) => toast.error(e.message),
    })
  }

  const removeId = async (id: string) => {
    const confirmed = await confirm({
      title: 'Remove entry?',
      description: `Remove '${id}' from this list?`,
      confirmLabel: 'Remove',
    })
    if (!confirmed) return
    setList.mutate(
      ids.filter((existing) => existing !== id),
      { onError: (e) => toast.error(e.message) },
    )
  }

  return (
    <div className="flex flex-col gap-3">
      {dialog}
      {ids.length === 0 && <p className="text-sm text-muted-foreground">No entries.</p>}
      <div className="flex flex-col gap-1">
        {ids.map((id) => (
          <div key={id} className="flex items-center justify-between rounded-md border px-3 py-2">
            <span className="font-mono text-sm">{id}</span>
            <Button size="icon" variant="ghost" onClick={() => removeId(id)}>
              <X className="size-4" />
            </Button>
          </div>
        ))}
      </div>
      <div className="flex gap-2">
        <Input
          placeholder="17-digit SteamID64…"
          value={newId}
          onChange={(e) => setNewId(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && addId()}
        />
        <Button onClick={addId} disabled={!newId.trim()}>
          Add
        </Button>
      </div>
    </div>
  )
}
