import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { api } from '@/lib/api-client'
import type { ManagedInstanceView } from '@/lib/types'

export function ManageInstanceDialog({ instance, onNavigate }: { instance: ManagedInstanceView; onNavigate?: (path: string) => void }) {
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [tags, setTags] = useState('')
  const [keepBackups, setKeepBackups] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const client = useQueryClient()
  const byId = `/instances/${instance.id}`
  const action = useMutation({
    mutationFn: async (operation: 'rename' | 'clone' | 'tags' | 'delete') => {
      if (operation === 'delete') await api.delete(`${byId}?keep_backups=${keepBackups}`)
      else if (operation === 'tags') await api.put(`${byId}/tags`, { tags: tags.split(',').map((tag) => tag.trim()).filter(Boolean) })
      else if (operation === 'rename') await api.post(`${byId}/rename`, { new_name: name.trim() })
      else {
        const cloned = await api.post<ManagedInstanceView>(`${byId}/rust/clone`, { name: name.trim(), world_name: name.trim() })
        return { operation, cloned }
      }
      return { operation, cloned: undefined }
    },
    onSuccess: ({ operation, cloned }) => {
      client.invalidateQueries({ queryKey: ['managed-instances'] })
      client.invalidateQueries({ queryKey: ['instances'] })
      setOpen(false)
      toast.success('Instance updated')
      if (operation === 'delete') onNavigate?.('/instances')
      if (operation === 'rename') onNavigate?.(`/instance/${instance.id}`)
      if (operation === 'clone' && cloned) onNavigate?.(`/instance/${cloned.id}`)
    },
    onError: (error) => toast.error(error.message),
  })
  return <Dialog open={open} onOpenChange={(value) => { setOpen(value); if (value) { setTags(instance.tags.join(', ')); setName(''); setConfirmDelete(false); setKeepBackups(false) } }}>
    <DialogTrigger render={<Button size="sm" variant="outline">Manage</Button>} />
    <DialogContent>
      <DialogHeader><DialogTitle>Manage {instance.name}</DialogTitle></DialogHeader>
      <div className="flex flex-col gap-3">
        <Label htmlFor={`tags-${instance.id}`}>Tags (comma separated)</Label>
        <Input id={`tags-${instance.id}`} value={tags} onChange={(e) => setTags(e.target.value)} placeholder="community, modded" />
        <Button variant="outline" disabled={action.isPending} onClick={() => action.mutate('tags')}>Save tags</Button>
        <Label htmlFor={`name-${instance.id}`}>New name</Label>
        <Input id={`name-${instance.id}`} value={name} onChange={(e) => setName(e.target.value)} placeholder="my-server" />
        <p className="text-xs text-muted-foreground">Cloning a Rust server copies configuration into a new server with its own ports and world.</p>
        <div className="flex gap-2">
          <Button disabled={instance.running || !name.trim() || action.isPending} onClick={() => action.mutate('rename')}>Rename</Button>
          <Button variant="outline" disabled={instance.game !== 'rust' || !name.trim() || action.isPending} onClick={() => action.mutate('clone')}>Clone configuration</Button>
        </div>
        <div className="flex flex-col gap-3 border-t pt-3">
          <p className="text-sm">Deleting permanently removes this server and its world. Stop it first.</p>
          <label className="flex items-center gap-2 text-sm"><Checkbox checked={keepBackups} onCheckedChange={setKeepBackups} />Keep backup archives</label>
          <label className="flex items-center gap-2 text-sm"><Checkbox checked={confirmDelete} onCheckedChange={setConfirmDelete} />I confirm deletion of {instance.name}</label>
          <Button variant="destructive" disabled={instance.running || !confirmDelete || action.isPending} onClick={() => action.mutate('delete')}>Delete server</Button>
        </div>
      </div>
    </DialogContent>
  </Dialog>
}
