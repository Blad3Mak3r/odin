import { Select } from '@base-ui/react/select'
import { Check, ChevronsUpDown } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { GameIcon } from '@/components/GameIcon'
import { CurseForgeModPicker } from '@/components/instance/CurseForgeModPicker'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { gameLabel } from '@/lib/games'
import { useCreateManagedInstance } from '@/lib/queries'
import type { Cs2GameMode, GameId, GameView } from '@/lib/types'

export function CreateManagedInstanceDialog({ games, defaultGame, variant }: { games: GameView[]; defaultGame?: GameId; variant?: 'default' | 'outline' }) {
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [game, setGame] = useState<GameId | null>(defaultGame ?? null)
  const [installMode, setInstallMode] = useState<'isolated' | 'shared'>('isolated')
  const [map, setMap] = useState('TheIsland_WP')
  const [sessionName, setSessionName] = useState('')
  const [mods, setMods] = useState('')
  const [cs2Map, setCs2Map] = useState('de_dust2')
  const [cs2Mode, setCs2Mode] = useState<Cs2GameMode>('competitive')
  const [cs2MaxPlayers, setCs2MaxPlayers] = useState('10')
  const [cs2Gslt, setCs2Gslt] = useState('')
  const create = useCreateManagedInstance()
  const selectedGame = games.some(({ id }) => id === game) ? game : (games[0]?.id ?? null)
  const isArk = selectedGame === 'ark-asa'
  const isCs2 = selectedGame === 'cs2'
  const supportsSharedInstall = games.find(({ id }) => id === selectedGame)?.capabilities.shared_install ?? false
  const submit = () => selectedGame && create.mutate({ game: selectedGame, name, ...(supportsSharedInstall ? { install_mode: installMode } : {}), ...(isArk ? { ark_launch: { map, session_name: sessionName || name, mods: mods.split(',').map((id) => Number(id.trim())).filter((id) => Number.isSafeInteger(id) && id > 0) } } : {}), ...(isCs2 ? { cs2_launch: { map: cs2Map, game_mode: cs2Mode, max_players: Number(cs2MaxPlayers), ...(cs2Gslt.trim() ? { gslt: cs2Gslt.trim() } : {}) } } : {}) }, {
    onSuccess: () => {
      setOpen(false)
      setName('')
      toast.success(`${gameLabel(selectedGame)} instance '${name}' created`)
    },
    onError: (error) => toast.error(error.message),
  })

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button variant={variant}>New instance</Button>} />
      <DialogContent>
        <DialogHeader><DialogTitle>Create instance</DialogTitle></DialogHeader>
        <FieldGroup>
          <Field>
            <Select.Root value={selectedGame} onValueChange={(value) => value && setGame(value as GameId)}>
              <Select.Label className="text-sm font-medium">Game</Select.Label>
              <Select.Trigger className="flex h-9 w-full items-center justify-between gap-2 rounded-none border bg-background px-3 text-sm outline-none hover:bg-accent focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50">
                <Select.Value className="flex items-center gap-2 whitespace-nowrap">
                  {(value: GameId | null) => {
                    const selected = games.find(({ id }) => id === value)
                    return selected ? <><GameIcon game={selected.id} />{selected.name}</> : 'Select a game'
                  }}
                </Select.Value>
                <Select.Icon><ChevronsUpDown className="size-4 text-muted-foreground" /></Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Positioner sideOffset={4} className="z-50">
                  <Select.Popup className="w-max min-w-[var(--anchor-width)] overflow-hidden rounded-none border bg-popover p-1 text-popover-foreground outline-none">
                    <Select.List>
                      {games.map((availableGame) => (
                        <Select.Item key={availableGame.id} value={availableGame.id} className="grid cursor-default grid-cols-[1rem_1fr] items-center gap-2 rounded-none px-2 py-1.5 text-sm outline-none data-highlighted:bg-accent data-highlighted:text-accent-foreground">
                          <Select.ItemIndicator><Check className="size-4" /></Select.ItemIndicator>
                          <Select.ItemText className="flex items-center gap-2 whitespace-nowrap"><GameIcon game={availableGame.id} />{availableGame.name}</Select.ItemText>
                        </Select.Item>
                      ))}
                    </Select.List>
                  </Select.Popup>
                </Select.Positioner>
              </Select.Portal>
            </Select.Root>
          </Field>
          {supportsSharedInstall && <Field><FieldLabel htmlFor="install-mode">Install mode</FieldLabel><select id="install-mode" className="h-9 border bg-background px-3 text-sm" value={installMode} onChange={(event) => setInstallMode(event.target.value as 'isolated' | 'shared')}><option value="isolated">Isolated</option><option value="shared">Shared</option></select></Field>}{isArk && <><Field><FieldLabel htmlFor="ark-map">Map</FieldLabel><Input id="ark-map" value={map} onChange={(event) => setMap(event.target.value)} /></Field><Field><FieldLabel htmlFor="ark-session-name">Session name</FieldLabel><Input id="ark-session-name" placeholder={name || 'my-server'} value={sessionName} onChange={(event) => setSessionName(event.target.value)} /></Field><Field><FieldLabel htmlFor="ark-mods">Mods</FieldLabel><CurseForgeModPicker id="ark-mods" value={mods} onChange={setMods} /></Field></>}{isCs2 && <><Field><FieldLabel htmlFor="cs2-map">Start map</FieldLabel><Input id="cs2-map" value={cs2Map} onChange={(event) => setCs2Map(event.target.value)} /></Field><Field><FieldLabel htmlFor="cs2-mode">Game mode</FieldLabel><select id="cs2-mode" className="h-9 border bg-background px-3 text-sm" value={cs2Mode} onChange={(event) => setCs2Mode(event.target.value as Cs2GameMode)}><option value="casual">Casual</option><option value="competitive">Competitive</option><option value="wingman">Wingman</option><option value="deathmatch">Deathmatch</option></select></Field><Field><FieldLabel htmlFor="cs2-max-players">Max players</FieldLabel><Input id="cs2-max-players" type="number" min={1} max={64} value={cs2MaxPlayers} onChange={(event) => setCs2MaxPlayers(event.target.value)} /></Field><Field><FieldLabel htmlFor="cs2-gslt">Game server login token</FieldLabel><Input id="cs2-gslt" type="password" placeholder="Optional; needed to be listed publicly" value={cs2Gslt} onChange={(event) => setCs2Gslt(event.target.value)} /></Field></>}
          <Field>
            <FieldLabel htmlFor="managed-instance-name">Name</FieldLabel>
            <Input id="managed-instance-name" placeholder="my-server" value={name} onChange={(event) => setName(event.target.value)} onKeyDown={(event) => event.key === 'Enter' && name && selectedGame && submit()} />
          </Field>
        </FieldGroup>
        <DialogFooter><Button disabled={!name || !selectedGame || create.isPending} onClick={submit}>Create</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
