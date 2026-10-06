import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { useExecuteSevenDaysConsole } from '@/lib/queries'

export function SevenDaysConsoleTab({ id, running }: { id: string; running: boolean }) {
  const execute = useExecuteSevenDaysConsole()
  const [command, setCommand] = useState('')
  const [output, setOutput] = useState('')
  const submit = () => {
    const value = command.trim()
    if (!value) return
    execute.mutate({ id, command: value }, { onSuccess: (response) => setOutput(response.output), onError: (error) => toast.error(error.message) })
  }
  return <Card><CardHeader><CardTitle>Server console</CardTitle><CardDescription>Commands are sent from Odin to the password-protected local 7 Days to Die console. Use <code>help</code> to inspect commands available on this server.</CardDescription></CardHeader><CardContent className="flex flex-col gap-3"><div className="flex gap-2"><Input value={command} disabled={!running || execute.isPending} placeholder="say Server restart in five minutes" onChange={(event) => setCommand(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); submit() } }} /><Button disabled={!running || execute.isPending || !command.trim()} onClick={submit}>Run</Button></div>{!running && <p className="text-sm text-muted-foreground">Start the server before using its console.</p>}{output && <pre className="max-h-96 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap">{output}</pre>}</CardContent></Card>
}
