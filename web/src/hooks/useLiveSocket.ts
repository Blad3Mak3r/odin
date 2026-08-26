import type { QueryClient } from '@tanstack/react-query'
import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef } from 'react'
import { apiWebSocketUrl } from '@/lib/api-client'
import type {
  ActivityEvent,
  HostResources,
  InstanceResources,
  InstanceView,
  PlayerInfo,
  ResourceSample,
  ResourcesTick,
} from '@/lib/types'

const ACTIVITY_FEED_CAP = 200
const HISTORY_CAP = 120
const RECONNECT_BASE_DELAY_MS = 1_000
const RECONNECT_MAX_DELAY_MS = 15_000

type WireEvent =
  | { type: 'activity'; event: ActivityEvent }
  | { type: 'resources'; tick: ResourcesTick }
  | { type: 'lagged'; skipped: number }

/// Keeps a single global WebSocket open for the lifetime of the app (mount
/// this once, from `AppShell`) and pushes live activity/resource updates
/// straight into the React Query cache, so pages just read them like any
/// other query instead of each managing their own socket or polling.
export function useLiveSocket() {
  const queryClient = useQueryClient()
  const reconnectAttempt = useRef(0)

  useEffect(() => {
    let socket: WebSocket | null = null
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null
    let closedByEffect = false

    const connect = () => {
      socket = new WebSocket(apiWebSocketUrl('/events/ws'))

      socket.onopen = () => {
        reconnectAttempt.current = 0
      }

      socket.onmessage = (event: MessageEvent<string>) => {
        let parsed: WireEvent
        try {
          parsed = JSON.parse(event.data) as WireEvent
        } catch (err) {
          console.error('failed to parse live event', err)
          return
        }
        applyWireEvent(queryClient, parsed)
      }

      socket.onclose = () => {
        if (closedByEffect) return
        const delay = Math.min(
          RECONNECT_BASE_DELAY_MS * 2 ** reconnectAttempt.current,
          RECONNECT_MAX_DELAY_MS,
        )
        reconnectAttempt.current += 1
        reconnectTimer = setTimeout(connect, delay)
      }

      socket.onerror = () => {
        socket?.close()
      }
    }

    connect()

    return () => {
      closedByEffect = true
      if (reconnectTimer) clearTimeout(reconnectTimer)
      socket?.close()
    }
  }, [queryClient])
}

function appendCapped<T>(prev: T[] | undefined, next: T, cap: number): T[] {
  const list = prev ? [...prev, next] : [next]
  return list.length > cap ? list.slice(list.length - cap) : list
}

function applyWireEvent(queryClient: QueryClient, event: WireEvent) {
  if (event.type === 'lagged') {
    return
  }

  if (event.type === 'activity') {
    queryClient.setQueryData<ActivityEvent[]>(['activity-feed'], (prev = []) => {
      const next = [event.event, ...prev]
      return next.length > ACTIVITY_FEED_CAP ? next.slice(0, ACTIVITY_FEED_CAP) : next
    })
    return
  }

  applyResourcesTick(queryClient, event.tick)
}

function applyResourcesTick(queryClient: QueryClient, tick: ResourcesTick) {
  const at = new Date().toISOString()

  queryClient.setQueryData<HostResources>(['resources', 'host'], tick.host)
  queryClient.setQueryData<ResourceSample[]>(['resource-history', 'host'], (prev) =>
    appendCapped(
      prev,
      { at, cpu_percent: tick.host.cpu_percent, memory_bytes: tick.host.memory_used_bytes },
      HISTORY_CAP,
    ),
  )

  const runningByName = new Map(tick.instances.map((entry) => [entry.name, entry.running]))
  queryClient.setQueryData<InstanceView[]>(['instances'], (prev) =>
    prev?.map((instance) =>
      runningByName.has(instance.name)
        ? { ...instance, running: runningByName.get(instance.name)! }
        : instance,
    ),
  )

  for (const entry of tick.instances) {
    const resources: InstanceResources = {
      running: entry.running,
      cpu_percent: entry.cpu_percent,
      memory_bytes: entry.memory_bytes,
    }
    queryClient.setQueryData(['resources', 'instance', entry.name], resources)
    queryClient.setQueryData<PlayerInfo[]>(['players', entry.name], entry.players)

    if (entry.running) {
      queryClient.setQueryData<ResourceSample[]>(
        ['resource-history', 'instance', entry.name],
        (prev) =>
          appendCapped(
            prev,
            { at, cpu_percent: entry.cpu_percent, memory_bytes: entry.memory_bytes },
            HISTORY_CAP,
          ),
      )
    }
  }
}
