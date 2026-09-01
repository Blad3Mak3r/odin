import { useEffect, useState } from 'react'
import type { JobStatus } from '@/lib/types'

type WireEvent =
  | { type: 'log'; line: string }
  | { type: 'status'; status: JobStatus }
  | { type: 'lagged'; skipped: number }

export function useJobSocket(jobId: string | null) {
  const [log, setLog] = useState<string[]>([])
  const [status, setStatus] = useState<JobStatus | null>(null)
  const [connected, setConnected] = useState(false)

  // Reset when switching jobs. Comparing against the previous id during
  // render (instead of an effect) avoids an extra commit.
  const [prevJobId, setPrevJobId] = useState(jobId)
  if (jobId !== prevJobId) {
    setPrevJobId(jobId)
    setLog([])
    setStatus(null)
    setConnected(false)
  }

  useEffect(() => {
    if (!jobId) return

    const source = new EventSource(`/api/jobs/${jobId}/sse`)

    source.onopen = () => setConnected(true)
    source.onerror = () => setConnected(false)
    source.onmessage = (event: MessageEvent<string>) => {
      let parsed: WireEvent
      try {
        parsed = JSON.parse(event.data) as WireEvent
      } catch (err) {
        console.error('failed to parse job SSE message', err)
        return
      }

      if (parsed.type === 'log') {
        setLog((prev) => [...prev, parsed.line])
      } else if (parsed.type === 'status') {
        setStatus(parsed.status)
      } else {
        setLog((prev) => [
          ...prev,
          `… missed ${parsed.skipped} log line(s), buffer overflowed …`,
        ])
      }
    }

    return () => {
      source.close()
      setConnected(false)
    }
  }, [jobId])

  return { log, status, connected }
}
