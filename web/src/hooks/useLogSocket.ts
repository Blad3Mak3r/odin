import { useEffect, useState } from 'react'

const MAX_LINES = 1000

export function useLogSocket(instanceName: string) {
  const [lines, setLines] = useState<string[]>([])
  const [connected, setConnected] = useState(false)

  // Reset when switching instances. Comparing against the previous name
  // during render (instead of an effect) avoids an extra commit.
  const [prevInstanceName, setPrevInstanceName] = useState(instanceName)
  if (instanceName !== prevInstanceName) {
    setPrevInstanceName(instanceName)
    setLines([])
  }

  useEffect(() => {
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
