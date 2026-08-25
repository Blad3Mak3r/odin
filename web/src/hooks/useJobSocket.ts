import { useEffect, useState } from 'react'
import { apiWebSocketUrl } from '@/lib/api-client'
import type { JobStatus } from '@/lib/types'

type WireEvent =
  | { type: 'log'; line: string }
  | { type: 'status'; status: JobStatus }
  | { type: 'lagged'; skipped: number }

export function useJobSocket(jobId: string | null) {
  const [log, setLog] = useState<string[]>([])
  const [status, setStatus] = useState<JobStatus | null>(null)
  const [connected, setConnected] = useState(false)

  useEffect(() => {
    if (!jobId) {
      setLog([])
      setStatus(null)
      setConnected(false)
      return
    }

    setLog([])
    setStatus(null)
    const socket = new WebSocket(apiWebSocketUrl(`/jobs/${jobId}/ws`))

    socket.onopen = () => setConnected(true)
    socket.onclose = () => setConnected(false)
    socket.onerror = () => setConnected(false)
    socket.onmessage = (event: MessageEvent<string>) => {
      let parsed: WireEvent
      try {
        parsed = JSON.parse(event.data) as WireEvent
      } catch (err) {
        console.error('failed to parse job WS message', err)
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
      socket.close()
      setConnected(false)
    }
  }, [jobId])

  return { log, status, connected }
}
