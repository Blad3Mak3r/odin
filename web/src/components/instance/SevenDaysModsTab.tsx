import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { useRemoveSevenDaysMod, useSevenDaysMods, useUploadSevenDaysMod } from '@/lib/queries'
import { ApiError } from '@/lib/api-client'

export function SevenDaysModsTab({ id, running }: { id: string; running: boolean }) {
  const mods = useSevenDaysMods(id)
  const upload = useUploadSevenDaysMod()
  const remove = useRemoveSevenDaysMod()
  const [file, setFile] = useState<File | null>(null)
  const { confirm, dialog } = useConfirmDialog()
  const submit = (replace = false) => {
    if (!file) return
    upload.mutate({ id, file, replace }, {
      onSuccess: () => { setFile(null); toast.success('Mod upload started') },
      onError: async (error) => {
        if (error instanceof ApiError && error.status === 409 && await confirm({ title: 'Replace installed mod?', description: error.message, confirmLabel: 'Replace' })) submit(true)
        else toast.error(error.message)
      },
    })
  }
  const removeMod = async (name: string, displayName: string) => {
    if (running) return
    const confirmed = await confirm({
      title: `Remove '${displayName}'?`,
      description: `Uninstall '${displayName}' from this server. You can upload it again later.`,
      confirmLabel: 'Remove mod',
    })
    if (confirmed) {
      remove.mutate(
        { id, modName: name },
        { onSuccess: () => toast.success('Mod removed'), onError: (error) => toast.error(error.message) },
      )
    }
  }
  return <section className="flex flex-col gap-4">
    {dialog}
    <div><h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">7 Days to Die mods</h2><p className="mt-1 text-sm text-muted-foreground">Each ZIP must contain one mod directory with ModInfo.xml. Stop the server before changing mods.</p></div>
    <section className="flex flex-col gap-3 rounded-lg border p-4"><div><h3 className="font-medium">Upload mod ZIP</h3><p className="mt-1 text-sm text-muted-foreground">Install a ZIP containing one mod directory with ModInfo.xml.</p></div><div className="flex flex-wrap items-center gap-2"><Input className="max-w-md" type="file" accept=".zip" disabled={running || upload.isPending} onChange={(event) => setFile(event.target.files?.[0] ?? null)} /><Button disabled={!file || running || upload.isPending} onClick={() => submit()}>Upload mod ZIP</Button></div>{running && <p className="text-sm text-muted-foreground">Stop the server before uploading a mod.</p>}</section>
    <section className="flex flex-col gap-3"><h3 className="font-medium">Installed mods</h3>{mods.data?.length === 0 && <p className="text-sm text-muted-foreground">No mods installed.</p>}{mods.data?.map((mod) => <div key={mod.name} className="flex flex-wrap items-start justify-between gap-3 rounded-lg border p-3"><div><p className="font-medium">{mod.display_name}</p><p className="text-sm text-muted-foreground">{mod.name} · v{mod.version}{mod.author ? ` · ${mod.author}` : ''}</p>{mod.description && <p className="mt-1 text-sm">{mod.description}</p>}</div><Button variant="destructive" size="sm" disabled={running || remove.isPending} onClick={() => removeMod(mod.name, mod.display_name)}>Remove</Button></div>)}</section>
  </section>
}
