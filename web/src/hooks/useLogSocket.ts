import { useEffect, useState } from 'react'
const MAX_LINES = 1000

export function useLogSocket(id: string) {
  const [lines, setLines] = useState<string[]>([])
  const [connected, setConnected] = useState(false)

  // Reset when switching instances. Comparing against the previous name
  // during render (instead of an effect) avoids an extra commit.
  const [previousId, setPreviousId] = useState(id)
  if (id !== previousId) {
    setPreviousId(id)
    setLines([])
  }

  useEffect(() => {
    if (!id) return
    const source = new EventSource(`/api/instances/${id}/logs/sse`)

    source.onopen = () => setConnected(true)
    source.onerror = () => setConnected(false)
    source.onmessage = (event: MessageEvent<string>) => {
      setLines((prev) => {
        const next = [...prev, event.data]
        return next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next
      })
    }

    return () => {
      source.close()
    }
  }, [id])

  return { lines, connected }
}
