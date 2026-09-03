import type { QueryClient } from '@tanstack/react-query'
import { useQueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import type {
  ActivityEvent,
  HostResources,
  InstanceResources,
  InstanceTransitions,
  InstanceView,
  ManagedInstanceView,
  PlayerInfo,
  ResourceSample,
  ResourcesTick,
} from '@/lib/types'

const ACTIVITY_FEED_CAP = 200
const HISTORY_CAP = 120

type WireEvent =
  | { type: 'activity'; event: ActivityEvent }
  | { type: 'resources'; tick: ResourcesTick }
  | { type: 'transitions'; transitions: InstanceTransitions }
  | { type: 'lagged'; skipped: number }

/// Keeps a single global SSE connection open for the lifetime of the app
/// (mount this once, from `AppShell`) and pushes live activity/resource
/// updates straight into the React Query cache, so pages just read them like
/// any other query instead of each managing their own connection or polling.
/// `EventSource` reconnects on its own after a drop, so there's no manual
/// backoff to manage here.
export function useLiveSocket() {
  const queryClient = useQueryClient()

  useEffect(() => {
    const source = new EventSource('/api/events/sse')

    source.onmessage = (event: MessageEvent<string>) => {
      let parsed: WireEvent
      try {
        parsed = JSON.parse(event.data) as WireEvent
      } catch (err) {
        console.error('failed to parse live event', err)
        return
      }
      applyWireEvent(queryClient, parsed)
    }

    return () => {
      source.close()
    }
  }, [queryClient])
}

function appendCapped<T>(prev: T[] | undefined, next: T, cap: number): T[] {
  const list = prev ? [...prev, next] : [next]
  return list.length > cap ? list.slice(list.length - cap) : list
}

function samePlayers(prev: PlayerInfo[] | undefined, next: PlayerInfo[]): boolean {
  if (!prev || prev.length !== next.length) return false
  return prev.every((p, i) => p.name === next[i].name && p.connected_at === next[i].connected_at)
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
  if (event.type === 'transitions') {
    const previous = queryClient.getQueryData<InstanceTransitions>(['instance-transitions']) ?? {}
    queryClient.setQueryData(['instance-transitions'], event.transitions)

    const transitionCompleted = Object.keys(previous).some(
      (name) => !(name in event.transitions),
    )
    if (transitionCompleted) {
      queryClient.invalidateQueries({ queryKey: ['instances'] })
      queryClient.invalidateQueries({ queryKey: ['version'] })
    }
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

  const valheimEntries = tick.instances.filter((entry) => entry.game === 'valheim')
  const runningByName = new Map(valheimEntries.map((entry) => [entry.name, entry.running]))
  // Only allocate a new array/objects for instances whose `running` flag
  // actually flipped, and bail out entirely if none did — otherwise every
  // row subscribed to `['instances']` (the whole instances table) re-renders
  // on every tick even when nothing visible changed.
  queryClient.setQueryData<InstanceView[]>(['instances'], (prev) => {
    if (!prev) return prev
    let changed = false
    const next = prev.map((instance) => {
      const running = runningByName.get(instance.name)
      if (running === undefined || running === instance.running) return instance
      changed = true
      return { ...instance, running }
    })
    return changed ? next : prev
  })

  for (const entry of tick.instances) {
    const resources: InstanceResources = {
      running: entry.running,
      ready: entry.ready,
      cpu_percent: entry.cpu_percent,
      memory_bytes: entry.memory_bytes,
    }
    if (entry.game === 'rust') {
      queryClient.setQueryData<ManagedInstanceView[]>(['managed-instances'], (prev) => {
        if (!prev) return prev
        let changed = false
        const next = prev.map((instance) => {
          if (instance.game !== entry.game || instance.name !== entry.name || instance.running === entry.running) return instance
          changed = true
          return { ...instance, running: entry.running }
        })
        return changed ? next : prev
      })
      queryClient.setQueryData<ManagedInstanceView>(['managed-instances', entry.game, entry.name], (prev) =>
        prev && prev.running !== entry.running ? { ...prev, running: entry.running } : prev,
      )
      queryClient.setQueryData(['managed-instances', 'rust', entry.name, 'resources'], resources)
      continue
    }

    queryClient.setQueryData(['resources', 'instance', entry.name], resources)
    // Same idea for the player list: keep the previous reference when the
    // set of connected players hasn't changed, so `PlayersBadge` (rendered
    // once per instance row) doesn't re-render every tick while idle.
    queryClient.setQueryData<PlayerInfo[]>(['players', entry.name], (prev) =>
      samePlayers(prev, entry.players) ? prev : entry.players,
    )
    queryClient.setQueryData<string | null>(['last-saved', entry.name], entry.last_saved_at)

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
