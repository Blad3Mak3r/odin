import { useState } from 'react'
import { toast } from 'sonner'
import { CurseForgeModPicker } from '@/components/instance/CurseForgeModPicker'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useUpdateGenericConfig } from '@/lib/queries'
import type { GenericConfigUpdateRequest } from '@/lib/types'

type ArkConfig = Pick<GenericConfigUpdateRequest, 'port' | 'query_port' | 'admin_port' | 'auto_restart'> & { mods?: number[] }

// Saves only the mod list: the config endpoint needs the ports and restart flag
// on every call, so they are re-sent unchanged, while the omitted map and
// session name keep their stored values.
export function ArkModsTab({ id, config, running }: { id: string; config: ArkConfig; running: boolean }) {
  const update = useUpdateGenericConfig()
  const initial = (config.mods ?? []).join(', ')
  const [mods, setMods] = useState(initial)

  const save = () => {
    const parsed = mods.trim() === '' ? [] : mods.split(',').map((mod) => Number(mod.trim()))
    if (parsed.some((mod) => !Number.isSafeInteger(mod) || mod <= 0)) {
      toast.error('Mod IDs must be positive whole numbers separated by commas')
      return
    }
    const { port, query_port, admin_port, auto_restart } = config
    update.mutate(
      { id, request: { port, query_port, admin_port, auto_restart, mods: parsed } },
      { onSuccess: () => toast.success('Mods saved'), onError: (error) => toast.error(error.message) },
    )
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Mods</CardTitle>
        <CardDescription>
          CurseForge mods this server loads, in order. {running ? 'Stop the server to change them.' : 'Changes apply the next time it starts.'}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <CurseForgeModPicker id="ark-mods" value={mods} disabled={running} onChange={setMods} />
        <Button className="w-fit" type="button" disabled={running || update.isPending || mods === initial} onClick={save}>
          Save mods
        </Button>
      </CardContent>
    </Card>
  )
}
