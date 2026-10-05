import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useMemo, useRef, useState } from 'react'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useLogSocket } from '@/hooks/useLogSocket'
import { useManagedInstanceLogs } from '@/lib/queries'
import type { GameId } from '@/lib/types'

const LINE_OPTIONS = [100, 200, 500, 1000]
const LIVE_TAIL_LINES = 200

export function ManagedLogsTab({ id, game, name }: { id?: string; game: GameId; name: string }) {
  const [lineCount, setLineCount] = useState(200)
  const [filter, setFilter] = useState('')
  const logs = useManagedInstanceLogs(game, name, lineCount, id)
  const socket = useLogSocket(name, game, id)
  const scrollRef = useRef<HTMLDivElement>(null)
  const showExpandedHistory = lineCount > LIVE_TAIL_LINES
  const live = !showExpandedHistory && (socket.connected || socket.lines.length > 0)
  const allLines = useMemo(
    () => (live ? socket.lines : (logs.data?.lines ?? [])),
    [live, socket.lines, logs.data?.lines],
  )
  const lines = useMemo(() => {
    const needle = filter.trim().toLowerCase()
    return needle ? allLines.filter((line) => line.toLowerCase().includes(needle)) : allLines
  }, [allLines, filter])
  const virtualizer = useVirtualizer({
    count: lines.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 18,
    overscan: 20,
  })

  useEffect(() => {
    if (lines.length > 0) virtualizer.scrollToIndex(lines.length - 1, { align: 'end' })
    // Scroll when new output arrives, not on every virtualizer identity change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lines.length])

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm text-muted-foreground">
          {live ? 'Live console log.' : `Last ${lineCount} lines (not live).`}
        </p>
        <div className="flex items-center gap-2">
          {live && <Badge variant={socket.connected ? 'default' : 'secondary'}>{socket.connected ? 'connected' : 'disconnected'}</Badge>}
          <div className="flex gap-1">
            {LINE_OPTIONS.map((count) => (
              <Button key={count} size="sm" variant={lineCount === count ? 'default' : 'outline'} onClick={() => setLineCount(count)}>
                {count}
              </Button>
            ))}
          </div>
        </div>
      </div>
      <Input placeholder="Filter lines…" value={filter} onChange={(event) => setFilter(event.target.value)} />
      <div ref={scrollRef} className="h-96 overflow-y-auto rounded-xl border bg-muted/30 p-3 font-mono text-xs">
        {logs.isLoading && !live && <p className="text-muted-foreground">Loading…</p>}
        {logs.isError && !live && <QueryError error={logs.error} />}
        {lines.length === 0 && <p className="text-muted-foreground">{filter ? 'No lines match the filter.' : 'No logs yet — start the instance first.'}</p>}
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map((item) => (
            <div
              key={item.key}
              data-index={item.index}
              ref={virtualizer.measureElement}
              className="absolute top-0 left-0 w-full whitespace-pre-wrap"
              style={{ transform: `translateY(${item.start}px)` }}
            >
              {lines[item.index]}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
