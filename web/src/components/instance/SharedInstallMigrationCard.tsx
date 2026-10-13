import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { gameLabel } from '@/lib/games'
import { useMoveToSharedInstall, useSharedInstall } from '@/lib/queries'
import type { ManagedInstanceView } from '@/lib/types'

export function SharedInstallMigrationCard({ instance }: { instance: ManagedInstanceView }) {
  const shared = useSharedInstall(instance.game, true)
  const move = useMoveToSharedInstall()
  const [deletePrivateFiles, setDeletePrivateFiles] = useState(false)
  const name = gameLabel(instance.game)
  const installed = shared.data?.installed ?? false
  const submit = () => {
    if (!window.confirm(`Move '${instance.name}' to the shared ${name} files? Its saves, settings and mods stay where they are.`)) return
    move.mutate({ id: instance.id, delete_private_files: deletePrivateFiles }, {
      onSuccess: (result) => toast.success(result.adopted_private_files ? 'This instance’s game files are now the shared installation' : 'Instance moved to the shared installation'),
      onError: (error) => toast.error(error.message),
    })
  }
  return (
    <Card>
      <CardHeader>
        <CardTitle>Game files</CardTitle>
        <CardDescription>
          This instance has its own copy of {name}. Moving it to the shared installation lets every shared instance use one copy. Saves, settings and mods stay in the instance. Stop the instance first.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <p className="text-sm text-muted-foreground">
          {shared.isLoading
            ? 'Checking the shared installation…'
            : installed
              ? 'The shared files are installed. This instance will start using them.'
              : 'No shared installation exists yet: this instance’s files become the shared installation, so nothing is downloaded.'}
        </p>
        {installed && (
          <div className="flex items-center justify-between rounded-xl border p-3">
            <div>
              <Label htmlFor="delete-private-files">Delete this instance’s own copy</Label>
              <p className="text-xs text-muted-foreground">Frees the disk space it uses. Off by default; nothing is deleted unless you ask.</p>
            </div>
            <Switch id="delete-private-files" checked={deletePrivateFiles} disabled={instance.running || move.isPending} onCheckedChange={setDeletePrivateFiles} />
          </div>
        )}
        <Button className="w-fit" type="button" disabled={instance.running || shared.isLoading || shared.data?.running || move.isPending} onClick={submit}>
          Move to shared installation
        </Button>
      </CardContent>
    </Card>
  )
}
