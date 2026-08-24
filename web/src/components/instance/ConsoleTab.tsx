import { useEffect, useRef, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useConsoleSocket } from '@/hooks/useConsoleSocket'

export function ConsoleTab({ name }: { name: string }) {
  const { lines, connected, sendCommand } = useConsoleSocket(name)
  const [command, setCommand] = useState('')
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight })
  }, [lines])

  const submit = () => {
    if (!command.trim()) return
    sendCommand(command)
    setCommand('')
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          Live console — commands are sent directly to the server process.
        </p>
        <Badge variant={connected ? 'default' : 'secondary'}>
          {connected ? 'connected' : 'disconnected'}
        </Badge>
      </div>

      <div
        ref={scrollRef}
        className="h-96 overflow-y-auto rounded-md border bg-muted/30 p-3 font-mono text-xs"
      >
        {lines.length === 0 && <p className="text-muted-foreground">No output yet.</p>}
        {lines.map((line, i) => (
          // Console lines have no stable id and never reorder, only append.
          // eslint-disable-next-line react/no-array-index-key
          <div key={i} className="whitespace-pre-wrap">
            {line}
          </div>
        ))}
      </div>

      <div className="flex gap-2">
        <Input
          placeholder="Type a server command and press Enter…"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && submit()}
          disabled={!connected}
        />
        <Button onClick={submit} disabled={!connected || !command.trim()}>
          Send
        </Button>
      </div>
    </div>
  )
}
