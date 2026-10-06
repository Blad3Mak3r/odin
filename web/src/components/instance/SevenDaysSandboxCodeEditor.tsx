import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

const V3_STANDARD_DEFAULT = 'AAAJABJACJADJARFBNC'

type SandboxRule = {
  code: string
  label: string
  values: string[]
  defaultValue: string
}

// This deliberately covers only a curated set of high-value rules. Unknown blocks stay in
// the code unchanged, so a newer game build or a mod cannot lose its settings.
const CURATED_RULES: SandboxRule[] = [
  { code: 'AS', label: 'XP multiplier', values: ['0', '0.25', '0.5', '0.75', '1', '1.25', '1.5', '1.75', '2', '3', '5'], defaultValue: '1' },
  { code: 'BE', label: 'Enemy spawning', values: ['Disabled', 'Enabled'], defaultValue: 'Enabled' },
  { code: 'BW', label: 'Blood moon frequency (days)', values: ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '14', '20', '30'], defaultValue: '7' },
  { code: 'DA', label: 'Loot abundance', values: ['0', '0.25', '0.35', '0.5', '0.65', '0.75', '0.85', '1', '1.25', '1.5', '2', '3', '4', '5'], defaultValue: '1' },
  { code: 'CO', label: 'Day length (minutes)', values: ['10', '20', '30', '40', '50', '60', '90', '120'], defaultValue: '60' },
  { code: 'CA', label: 'Airdrop frequency', values: ['0', '1', '2', '3', '4', '5', '6'], defaultValue: '3' },
]

function isValidCode(value: string) {
  return /^A(?:[A-Z]{3})*$/.test(value)
}

function encodedValue(code: string, rule: SandboxRule) {
  const blocks = code.slice(1).match(/.{3}/g) ?? []
  const block = blocks.find((entry) => entry.slice(0, 2) === rule.code)
  const index = block ? block.charCodeAt(2) - 65 : -1
  return rule.values[index] ?? rule.defaultValue
}

function updateRule(code: string, rule: SandboxRule, value: string) {
  const index = rule.values.indexOf(value)
  if (index < 0) return code
  const blocks = (isValidCode(code) ? code.slice(1).match(/.{3}/g) ?? [])
    .filter((entry) => entry.slice(0, 2) !== rule.code)
  blocks.push(`${rule.code}${String.fromCharCode(65 + index)}`)
  return `A${blocks.sort().join('')}`
}

export function SevenDaysSandboxCodeEditor({ value, disabled, onChange }: { value: string; disabled: boolean; onChange: (value: string) => void }) {
  const valid = !value || isValidCode(value)
  const blocks = value ? Math.max(0, (value.length - 1) / 3) : 0
  return <div className="flex flex-col gap-3 sm:col-span-2">
    <div className="flex flex-col gap-2">
      <Label htmlFor="7d2d-sandbox-code">Sandbox rules</Label>
      <Input id="7d2d-sandbox-code" value={value} disabled={disabled} placeholder="Paste a code copied from 7 Days to Die" onChange={(event) => onChange(event.target.value.trim().toUpperCase())} />
      {!valid ? <p className="text-xs text-destructive">A V3 SandboxCode starts with A and then uses groups of three uppercase letters.</p> : <p className="text-xs text-muted-foreground">{value ? `V3 code with ${blocks} encoded setting${blocks === 1 ? '' : 's'}.` : 'Empty: preserves the installed server default.'} The controls below preserve unrecognised settings in the code.</p>}
      <div className="flex flex-wrap gap-2"><Button type="button" size="sm" variant="outline" disabled={disabled} onClick={() => onChange(V3_STANDARD_DEFAULT)}>Use V3 standard default</Button><Button type="button" size="sm" variant="ghost" disabled={disabled || !value} onClick={() => onChange('')}>Use installed default</Button></div>
    </div>
    {valid && <div className="rounded-md border p-3">
      <p className="mb-3 text-sm font-medium">Common rules</p>
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {CURATED_RULES.map((rule) => <label key={rule.code} className="flex flex-col gap-1 text-sm font-medium">{rule.label}
          <select className="h-9 rounded-md border border-input bg-transparent px-3 text-sm shadow-xs outline-none disabled:cursor-not-allowed disabled:opacity-50" disabled={disabled} value={encodedValue(value || 'A', rule)} onChange={(event) => onChange(updateRule(value || 'A', rule, event.target.value))}>{rule.values.map((option) => <option key={option} value={option}>{option}</option>)}</select>
        </label>)}
      </div>
      <p className="mt-3 text-xs text-muted-foreground">Experimental V3 compatibility subset. Use the code field for rules not shown here or for a game version whose SandboxCode format differs.</p>
    </div>}
  </div>
}
