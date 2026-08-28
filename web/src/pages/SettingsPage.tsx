import { useState } from 'react'
import { toast } from 'sonner'
import { PageHeader } from '@/components/PageHeader'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { useClearNexusApiKey, useSetNexusApiKey, useSettings } from '@/lib/queries'

export function SettingsPage() {
  const settings = useSettings()
  const setApiKey = useSetNexusApiKey()
  const clearApiKey = useClearNexusApiKey()
  const [apiKey, setApiKeyInput] = useState('')

  const handleSave = () => {
    if (!apiKey.trim()) return
    setApiKey.mutate(apiKey.trim(), {
      onSuccess: () => {
        setApiKeyInput('')
        toast.success('Nexus Mods API key saved')
      },
      onError: (e) => toast.error(e.message),
    })
  }

  const handleClear = () => {
    clearApiKey.mutate(undefined, {
      onSuccess: () => toast.success('Nexus Mods API key cleared'),
      onError: (e) => toast.error(e.message),
    })
  }

  return (
    <div className="flex flex-col gap-8">
      <PageHeader title="Settings" description="Global configuration shared across all instances." />

      {settings.isError && <QueryError error={settings.error} />}

      <div className="flex max-w-md flex-col gap-4 rounded-xl border p-4">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium">Nexus Mods API key</h2>
          {settings.isLoading ? (
            <Skeleton className="h-5 w-20" />
          ) : (
            <Badge variant={settings.data?.nexus_api_key_configured ? 'default' : 'outline'}>
              {settings.data?.nexus_api_key_configured ? 'configured' : 'not configured'}
            </Badge>
          )}
        </div>
        <p className="text-xs text-muted-foreground">
          Used to look up and install mods from Nexus Mods. Get a personal API key from your{' '}
          <a
            href="https://www.nexusmods.com/users/myaccount?tab=api"
            target="_blank"
            rel="noreferrer"
            className="underline"
          >
            Nexus Mods account settings
          </a>
          .
        </p>

        <div className="flex flex-col gap-2">
          <Label htmlFor="nexus-api-key">API key</Label>
          <Input
            id="nexus-api-key"
            type="password"
            placeholder="•••••••••••••••••"
            value={apiKey}
            onChange={(e) => setApiKeyInput(e.target.value)}
          />
        </div>

        <div className="flex gap-2">
          <Button
            className="w-fit"
            disabled={!apiKey.trim() || setApiKey.isPending}
            onClick={handleSave}
          >
            Save
          </Button>
          {settings.data?.nexus_api_key_configured && (
            <Button
              variant="outline"
              className="w-fit"
              disabled={clearApiKey.isPending}
              onClick={handleClear}
            >
              Clear
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}
