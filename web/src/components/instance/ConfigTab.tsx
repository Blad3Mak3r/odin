import { useState } from 'react'
import { toast } from 'sonner'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { useConfig, useUpdateConfig } from '@/lib/queries'
import type { ConfigView } from '@/lib/types'

const MIN_PORT = 1
const MAX_PORT = 65535

export function ConfigTab({ name }: { name: string }) {
  const config = useConfig(name)
  const updateConfig = useUpdateConfig(name)

  if (config.isError) {
    return <QueryError error={config.error} />
  }

  if (config.isLoading || !config.data) {
    return (
      <div className="flex max-w-md flex-col gap-4">
        <Skeleton className="h-14 w-full" />
        <Skeleton className="h-14 w-full" />
        <Skeleton className="h-16 w-full" />
        <Skeleton className="h-12 w-full" />
      </div>
    )
  }

  // Keyed by instance name so switching instances mounts a fresh form
  // seeded from the newly loaded config, instead of syncing state in an
  // effect every time `config.data` changes (e.g. on refetch).
  return <ConfigForm key={name} initial={config.data} updateConfig={updateConfig} />
}

function ConfigForm({
  initial,
  updateConfig,
}: {
  initial: ConfigView
  updateConfig: ReturnType<typeof useUpdateConfig>
}) {
  const [world, setWorld] = useState(initial.world_name)
  const [port, setPort] = useState(String(initial.port))
  const [password, setPassword] = useState(initial.password ?? '')
  const [isPublic, setIsPublic] = useState(initial.public)
  const [autoRestart, setAutoRestart] = useState(initial.auto_restart)

  const portNumber = Number(port)
  const portInvalid = port.trim() === '' || Number.isNaN(portNumber) || portNumber < MIN_PORT || portNumber > MAX_PORT

  const handleSave = () => {
    if (portInvalid) return
    updateConfig.mutate(
      { world, port: portNumber, password, public: isPublic, auto_restart: autoRestart },
      {
        onSuccess: () => toast.success('Config updated — restart the instance to apply it'),
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <div className="flex max-w-md flex-col gap-4">
      <div className="flex flex-col gap-2">
        <Label htmlFor="world">World name</Label>
        <Input id="world" value={world} onChange={(e) => setWorld(e.target.value)} />
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="port">Port</Label>
        <Input id="port" type="number" value={port} onChange={(e) => setPort(e.target.value)} />
        {portInvalid && (
          <p className="text-xs text-destructive">Enter a valid port number ({MIN_PORT}-{MAX_PORT}).</p>
        )}
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="password">Password</Label>
        <Input id="password" value={password} onChange={(e) => setPassword(e.target.value)} />
        <p className="text-xs text-muted-foreground">At least 5 characters (Valheim's minimum).</p>
      </div>
      <div className="flex items-center justify-between rounded-xl border p-3">
        <Label htmlFor="public">Public</Label>
        <Switch id="public" checked={isPublic} onCheckedChange={setIsPublic} />
      </div>
      <div className="flex items-center justify-between rounded-xl border p-3">
        <div>
          <Label htmlFor="auto-restart">Restart automatically</Label>
          <p className="text-xs text-muted-foreground">
            If the server crashes, start it again without waiting for you to notice.
          </p>
        </div>
        <Switch id="auto-restart" checked={autoRestart} onCheckedChange={setAutoRestart} />
      </div>

      <Button className="w-fit" onClick={handleSave} disabled={updateConfig.isPending || portInvalid}>
        Save
      </Button>
      <p className="text-xs text-muted-foreground">
        Changes take effect the next time the instance is restarted.
      </p>
    </div>
  )
}
