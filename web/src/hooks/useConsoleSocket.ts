import { useCallback, useEffect, useRef, useState } from 'react'
import { apiWebSocketUrl } from '@/lib/api-client'

const MAX_LINES = 1000

export function useConsoleSocket(instanceName: string) {
  const [lines, setLines] = useState<string[]>([])
  const [connected, setConnected] = useState(false)
  const socketRef = useRef<WebSocket | null>(null)

  useEffect(() => {
    setLines([])
    const socket = new WebSocket(apiWebSocketUrl(`/instances/${instanceName}/console/ws`))
    socketRef.current = socket

    socket.onopen = () => setConnected(true)
    socket.onclose = () => setConnected(false)
    socket.onerror = () => setConnected(false)
    socket.onmessage = (event: MessageEvent<string>) => {
      setLines((prev) => {
        const next = [...prev, event.data]
        return next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next
      })
    }

    return () => {
      socket.close()
      socketRef.current = null
    }
  }, [instanceName])

  const sendCommand = useCallback((command: string) => {
    socketRef.current?.send(command)
  }, [])

  return { lines, connected, sendCommand }
}
