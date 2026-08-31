import { Loader2 } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { JobProgress } from '@/components/JobProgress'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { useJobSocket } from '@/hooks/useJobSocket'
import { useUploadMod } from '@/lib/queries'

export function UploadModForm({
  name,
  running = false,
}: {
  name: string
  running?: boolean
}) {
  const [modName, setModName] = useState('')
  const [version, setVersion] = useState('')
  const [file, setFile] = useState<File | null>(null)
  const [jobId, setJobId] = useState<string | null>(null)
  const uploadMod = useUploadMod()
  const job = useJobSocket(jobId)
  const { confirm, dialog } = useConfirmDialog()

  const canSubmit = modName.trim().length > 0 && file !== null && !uploadMod.isPending && !running

  const handleSubmit = async () => {
    if (!file) return
    const confirmed = await confirm({
      title: `Upload '${modName}'?`,
      description: `Install '${file.name}' on '${name}' as a mod named '${modName}'. Only upload mods you trust — the archive is extracted as-is.`,
      confirmLabel: 'Upload & install',
    })
    if (!confirmed) return

    uploadMod.mutate(
      { name, modName: modName.trim(), version, file },
      {
        onSuccess: (handle) => setJobId(handle.id),
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <div className="flex flex-col gap-3">
      {dialog}
      <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
        Upload a mod .zip
      </h2>
      <p className="text-xs text-muted-foreground">
        For mods not on Thunderstore or Nexus, or when a Nexus file can't be downloaded
        automatically — download it yourself, then upload the .zip here.
      </p>

      <div className="grid max-w-md gap-4">
        <div className="flex flex-col gap-2">
          <Label htmlFor="upload-mod-name">Name</Label>
          <Input
            id="upload-mod-name"
            placeholder="My Cool Mod"
            value={modName}
            onChange={(e) => setModName(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="upload-mod-version">Version (optional)</Label>
          <Input
            id="upload-mod-version"
            placeholder="1.0.0"
            value={version}
            onChange={(e) => setVersion(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="upload-mod-file">Mod .zip</Label>
          <Input
            id="upload-mod-file"
            type="file"
            accept=".zip"
            onChange={(e) => setFile(e.target.files?.[0] ?? null)}
          />
        </div>
        <Button className="w-fit" disabled={!canSubmit} onClick={handleSubmit}>
          {uploadMod.isPending && <Loader2 className="size-4 animate-spin" />}
          Upload & install
        </Button>
      </div>

      {jobId && <JobProgress log={job.log} status={job.status} connected={job.connected} />}
    </div>
  )
}
