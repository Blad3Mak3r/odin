import { useEffect, useState } from 'react'
const MAX_LINES = 1000

export type LogSource = 'console' | 'update'

export function useLogSocket(id: string, source: LogSource = 'console') {
  const [lines, setLines] = useState<string[]>([])
  const [connected, setConnected] = useState(false)

  // Reset when switching instances or log source. Comparing against the
  // previous key during render (instead of an effect) avoids an extra commit.
  const key = `${id}:${source}`
  const [previousKey, setPreviousKey] = useState(key)
  if (key !== previousKey) {
    setPreviousKey(key)
    setLines([])
  }

  useEffect(() => {
    if (!id) return
    const stream = new EventSource(`/api/instances/${id}/logs${source === 'update' ? '/update' : ''}/sse`)

    stream.onopen = () => setConnected(true)
    stream.onerror = () => setConnected(false)
    stream.onmessage = (event: MessageEvent<string>) => {
      setLines((prev) => {
        const next = [...prev, event.data]
        return next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next
      })
    }

    return () => {
      stream.close()
    }
  }, [id, source])

  return { lines, connected }
}
