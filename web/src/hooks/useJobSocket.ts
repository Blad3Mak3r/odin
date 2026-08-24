import { useEffect, useState } from 'react'
import { apiWebSocketUrl } from '@/lib/api-client'
import type { JobStatus } from '@/lib/types'

type WireEvent = { type: 'log'; line: string } | { type: 'status'; status: JobStatus }

export function useJobSocket(jobId: string | null) {
  const [log, setLog] = useState<string[]>([])
  const [status, setStatus] = useState<JobStatus | null>(null)

  useEffect(() => {
    if (!jobId) {
      setLog([])
      setStatus(null)
      return
    }

    setLog([])
    setStatus(null)
    const socket = new WebSocket(apiWebSocketUrl(`/jobs/${jobId}/ws`))

    socket.onmessage = (event: MessageEvent<string>) => {
      const parsed = JSON.parse(event.data) as WireEvent
      if (parsed.type === 'log') {
        setLog((prev) => [...prev, parsed.line])
      } else {
        setStatus(parsed.status)
      }
    }

    return () => socket.close()
  }, [jobId])

  return { log, status }
}
