import { useEffect, useRef, useState } from 'react'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { useLogs } from '@/lib/queries'

const LINE_OPTIONS = [100, 200, 500, 1000]

export function LogsTab({ name }: { name: string }) {
  const [lineCount, setLineCount] = useState(200)
  const logs = useLogs(name, lineCount)
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight })
  }, [logs.data])

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">Console log tail, refreshed every few seconds.</p>
        <div className="flex gap-1">
          {LINE_OPTIONS.map((n) => (
            <Button
              key={n}
              size="sm"
              variant={lineCount === n ? 'default' : 'outline'}
              onClick={() => setLineCount(n)}
            >
              {n}
            </Button>
          ))}
        </div>
      </div>

      <div
        ref={scrollRef}
        className="h-96 overflow-y-auto rounded-md border bg-muted/30 p-3 font-mono text-xs"
      >
        {logs.isLoading && <p className="text-muted-foreground">Loading…</p>}
        {logs.isError && <QueryError error={logs.error} />}
        {logs.data?.lines.length === 0 && (
          <p className="text-muted-foreground">No logs yet — start the instance first.</p>
        )}
        {logs.data?.lines.map((line, i) => (
          // eslint-disable-next-line react/no-array-index-key
          <div key={i} className="whitespace-pre-wrap">
            {line}
          </div>
        ))}
      </div>
    </div>
  )
}
