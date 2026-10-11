import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useExecuteRustRcon } from '@/lib/queries'

export function RustRconTab({ id, running }: { id: string; running: boolean }) {
  const execute = useExecuteRustRcon()
  const [command, setCommand] = useState('')
  const [output, setOutput] = useState<string | null>(null)

  const submit = () => execute.mutate(
    { id, command },
    {
      onSuccess: (response) => setOutput(response.output),
      onError: (error) => toast.error(error.message),
    },
  )

  return (
    <div className="flex flex-col gap-6">
      <Card>
        <CardHeader>
          <CardTitle>RCON console</CardTitle>
          <CardDescription>Run a Rust server command. Odin connects to WebRCON locally; the RCON password is never sent to this browser.</CardDescription>
        </CardHeader>
        <CardContent>
          <form
            className="flex flex-col gap-3 sm:flex-row sm:items-end"
            onSubmit={(event) => {
              event.preventDefault()
              if (!command.trim()) {
                toast.error('Enter a Rust command')
                return
              }
              submit()
            }}
          >
            <div className="flex flex-1 flex-col gap-2">
              <Label htmlFor="rust-rcon-command">Command</Label>
              <Input
                id="rust-rcon-command"
                placeholder="status"
                value={command}
                disabled={!running || execute.isPending}
                onChange={(event) => setCommand(event.target.value)}
                autoComplete="off"
              />
            </div>
            <Button type="submit" disabled={!running || execute.isPending}>{execute.isPending ? 'Running…' : 'Run command'}</Button>
          </form>
          {!running && <p className="mt-3 text-sm text-muted-foreground">Start this Rust server before sending RCON commands.</p>}
        </CardContent>
      </Card>
      {output !== null && (
        <Card>
          <CardHeader>
            <CardTitle>Response</CardTitle>
          </CardHeader>
          <CardContent>
            <pre className="max-h-96 overflow-auto whitespace-pre-wrap rounded-xl border bg-muted/30 p-3 font-mono text-xs">{output || '(no output)'}</pre>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
