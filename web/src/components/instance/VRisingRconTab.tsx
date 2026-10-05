import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useExecuteVRisingRcon } from '@/lib/queries'

export function VRisingRconTab({ id, running }: { id: string; running: boolean }) {
  const execute = useExecuteVRisingRcon()
  const [command, setCommand] = useState('')
  const [output, setOutput] = useState<string | null>(null)

  const submit = () => execute.mutate(
    { id, command },
    {
      onSuccess: (response) => setOutput(response.output),
      onError: (error) => toast.error(error.message),
    },
  )

  return <div className="flex flex-col gap-6">
    <Card>
      <CardHeader>
        <CardTitle>RCON console</CardTitle>
        <CardDescription>Run a V Rising command through its loopback-only Source RCON listener. The password never leaves Odin.</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="flex flex-col gap-3 sm:flex-row sm:items-end" onSubmit={(event) => {
          event.preventDefault()
          if (!command.trim()) {
            toast.error('Enter a V Rising command')
            return
          }
          submit()
        }}>
          <div className="flex flex-1 flex-col gap-2">
            <Label htmlFor="vrising-rcon-command">Command</Label>
            <Input id="vrising-rcon-command" placeholder="announce Welcome!" value={command} disabled={!running || execute.isPending} onChange={(event) => setCommand(event.target.value)} autoComplete="off" />
          </div>
          <Button type="submit" disabled={!running || execute.isPending}>{execute.isPending ? 'Running…' : 'Run command'}</Button>
        </form>
        {!running && <p className="mt-3 text-sm text-muted-foreground">Start this V Rising server before sending RCON commands.</p>}
      </CardContent>
    </Card>
    {output !== null && <Card><CardHeader><CardTitle>Response</CardTitle></CardHeader><CardContent><pre className="max-h-96 overflow-auto whitespace-pre-wrap rounded-xl border bg-muted/30 p-3 font-mono text-xs">{output || '(no output)'}</pre></CardContent></Card>}
  </div>
}
