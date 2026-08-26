import { useEffect, useState } from 'react'

const MAX_LINES = 1000

export function useLogSocket(instanceName: string) {
  const [lines, setLines] = useState<string[]>([])
  const [connected, setConnected] = useState(false)

  useEffect(() => {
    setLines([])
    const source = new EventSource(`/api/instances/${instanceName}/logs/sse`)

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
  }, [instanceName])

  return { lines, connected }
}
