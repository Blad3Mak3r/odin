import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

const V3_STANDARD_DEFAULT = 'AAAJABJACJADJARFBNC'

export function SevenDaysSandboxCodeEditor({ value, disabled, onChange }: { value: string; disabled: boolean; onChange: (value: string) => void }) {
  const valid = !value || (/^A(?:[A-Z]{3})*$/.test(value))
  const blocks = value ? Math.max(0, (value.length - 1) / 3) : 0
  return <div className="flex flex-col gap-2 sm:col-span-2"><Label htmlFor="7d2d-sandbox-code">Sandbox rules</Label><Input id="7d2d-sandbox-code" value={value} disabled={disabled} placeholder="Paste a code copied from 7 Days to Die" onChange={(event) => onChange(event.target.value.trim().toUpperCase())} />{!valid ? <p className="text-xs text-destructive">A V3 SandboxCode starts with A and then uses groups of three uppercase letters.</p> : <p className="text-xs text-muted-foreground">{value ? `V3 code with ${blocks} encoded setting${blocks === 1 ? '' : 's'}.` : 'Empty: preserves the installed server default.'} Odin keeps this compact code instead of guessing rules that can change between game versions.</p>}<div className="flex flex-wrap gap-2"><Button type="button" size="sm" variant="outline" disabled={disabled} onClick={() => onChange(V3_STANDARD_DEFAULT)}>Use V3 standard default</Button><Button type="button" size="sm" variant="ghost" disabled={disabled || !value} onClick={() => onChange('')}>Use installed default</Button></div></div>
}
