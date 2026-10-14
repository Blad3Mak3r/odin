import { useVirtualizer } from '@tanstack/react-virtual'
import { useEffect, useMemo, useRef, useState } from 'react'
import { AnsiText } from '@/components/instance/AnsiText'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useLogSocket, type LogSource } from '@/hooks/useLogSocket'
import { stripAnsi } from '@/lib/ansi'
import { parseLogLine, type LogEntry } from '@/lib/logLine'
import { useManagedInstanceLogFiles, useManagedInstanceLogs } from '@/lib/queries'

const LINE_OPTIONS = [100, 200, 500, 1000, 5000]
const LIVE_TAIL_LINES = 200

const timeFormat = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false })
const dateTimeFormat = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'medium' })

function LogRow({ entry }: { entry: LogEntry }) {
  return (
    <div className="flex items-start gap-3 border-b border-border/50 px-3 py-1.5 hover:bg-muted/60">
      <time
        dateTime={entry.time?.toISOString()}
        title={entry.time ? dateTimeFormat.format(entry.time) : 'No timestamp'}
        className="mt-0.5 w-[4.25rem] shrink-0 rounded bg-muted px-1.5 py-0.5 text-center text-[10px] leading-4 text-muted-foreground tabular-nums"
      >
        {entry.time ? timeFormat.format(entry.time) : '--:--:--'}
      </time>
      <div className="min-w-0 flex-1 text-[13px] leading-5 break-words whitespace-pre-wrap text-foreground">
        <AnsiText text={entry.message} />
      </div>
    </div>
  )
}

export function ManagedLogsTab({ id }: { id: string }) {
  const [lineCount, setLineCount] = useState(200)
  const [filter, setFilter] = useState('')
  const [source, setSource] = useState<LogSource>('console')
  // `null` follows today's live log; a date shows that archived day.
  const [date, setDate] = useState<string | null>(null)
  const installLog = source === 'update'
  const archived = !installLog && date !== null
  const logFiles = useManagedInstanceLogFiles(id)
  const logs = useManagedInstanceLogs(id, lineCount, archived ? date : undefined)
  const socket = useLogSocket(id, installLog ? 'update' : 'console')
  const scrollRef = useRef<HTMLDivElement>(null)
  const showExpandedHistory = !installLog && lineCount > LIVE_TAIL_LINES
  const live = !archived && (installLog || !showExpandedHistory) && (socket.connected || socket.lines.length > 0)
  const allLines = useMemo(
    () => (live || installLog ? socket.lines : (logs.data?.lines ?? [])),
    [live, installLog, socket.lines, logs.data?.lines],
  )
  const entries = useMemo(() => {
    const parsed = allLines.map(parseLogLine)
    const needle = filter.trim().toLowerCase()
    return needle ? parsed.filter((entry) => stripAnsi(entry.message).toLowerCase().includes(needle)) : parsed
  }, [allLines, filter])
  const currentDate = logFiles.data?.files.find((file) => file.current)?.date
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 32,
    overscan: 20,
  })

  useEffect(() => {
    if (entries.length > 0) virtualizer.scrollToIndex(entries.length - 1, { align: 'end' })
    // Scroll when new output arrives, not on every virtualizer identity change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entries.length, date])

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm text-muted-foreground">
          {installLog ? 'Live SteamCMD install log.' : archived ? `Archived log from ${date} (last ${lineCount} lines).` : live ? 'Live console log.' : `Last ${lineCount} lines (not live).`}
        </p>
        <div className="flex items-center gap-2">
          <div className="flex gap-1">
            <Button size="sm" variant={installLog ? 'outline' : 'default'} onClick={() => setSource('console')}>Console</Button>
            <Button size="sm" variant={installLog ? 'default' : 'outline'} onClick={() => setSource('update')}>SteamCMD</Button>
          </div>
          {!installLog && (logFiles.data?.files.length ?? 0) > 1 && (
            <select
              aria-label="Log day"
              className="h-8 rounded-md border bg-background px-2 text-sm"
              value={date ?? ''}
              onChange={(event) => setDate(event.target.value || null)}
            >
              <option value="">{currentDate ? `Current (${currentDate})` : 'Current'}</option>
              {logFiles.data?.files.filter((file) => !file.current).map((file) => (
                <option key={file.date} value={file.date}>{file.date}</option>
              ))}
            </select>
          )}
          {live && <Badge variant={socket.connected ? 'default' : 'secondary'}>{socket.connected ? 'connected' : 'disconnected'}</Badge>}
          {!installLog && <div className="flex gap-1">
            {LINE_OPTIONS.map((count) => (
              <Button key={count} size="sm" variant={lineCount === count ? 'default' : 'outline'} onClick={() => setLineCount(count)}>
                {count}
              </Button>
            ))}
          </div>}
        </div>
      </div>
      <Input placeholder="Filter lines…" value={filter} onChange={(event) => setFilter(event.target.value)} />
      <div ref={scrollRef} className="h-96 overflow-y-auto rounded-xl border bg-muted/30 font-mono text-xs">
        {!installLog && logs.isLoading && !live && <p className="p-3 text-muted-foreground">Loading…</p>}
        {!installLog && logs.isError && !live && <div className="p-3"><QueryError error={logs.error} /></div>}
        {entries.length === 0 && <p className="p-3 text-muted-foreground">{filter ? 'No lines match the filter.' : archived ? 'This day has no log lines.' : installLog ? 'No install output yet — it appears here while the server downloads on first start.' : 'No logs yet — start the instance first.'}</p>}
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map((item) => (
            <div
              key={item.key}
              data-index={item.index}
              ref={virtualizer.measureElement}
              className="absolute top-0 left-0 w-full"
              style={{ transform: `translateY(${item.start}px)` }}
            >
              <LogRow entry={entries[item.index]} />
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
