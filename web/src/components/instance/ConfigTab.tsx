import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { useConfig, useUpdateConfig } from '@/lib/queries'

const MIN_PORT = 1
const MAX_PORT = 65535

export function ConfigTab({ name }: { name: string }) {
  const config = useConfig(name)
  const updateConfig = useUpdateConfig(name)

  const [world, setWorld] = useState('')
  const [port, setPort] = useState('')
  const [password, setPassword] = useState('')
  const [isPublic, setIsPublic] = useState(true)

  useEffect(() => {
    if (!config.data) return
    setWorld(config.data.world_name)
    setPort(String(config.data.port))
    setPassword(config.data.password ?? '')
    setIsPublic(config.data.public)
  }, [config.data])

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

  const portNumber = Number(port)
  const portInvalid = port.trim() === '' || Number.isNaN(portNumber) || portNumber < MIN_PORT || portNumber > MAX_PORT

  const handleSave = () => {
    if (portInvalid) return
    updateConfig.mutate(
      { world, port: portNumber, password, public: isPublic },
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

      <Button className="w-fit" onClick={handleSave} disabled={updateConfig.isPending || portInvalid}>
        Save
      </Button>
      <p className="text-xs text-muted-foreground">
        Changes take effect the next time the instance is restarted.
      </p>
    </div>
  )
}
