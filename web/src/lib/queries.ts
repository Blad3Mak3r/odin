import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from './api-client'
import type {
  ActivityEvent,
  BackupEntry,
  BackupScheduleView,
  BackupStorageRequest,
  BackupStorageView,
  BepInExStatus,
  BulkBepInExResult,
  BulkResult,
  ChangelogRelease,
  CheckResult,
  ConfigFileEntry,
  ConfigFileView,
  ConfigUpdateRequest,
  ConfigView,
  GlobalMod,
  GameId,
  GameInstanceTransitions,
  GameView,
  GenericConfigUpdateRequest,
  HostResources,
  InstallStatusView,
  InstanceResources,
  InstanceDefaults,
  InstanceTransition,
  InstanceTransitions,
  InstanceView,
  ManagedInstanceView,
  JobHandle,
  JobSummary,
  LastExitInfo,
  ListKind,
  ListView,
  LogsView,
  ModSearchResult,
  PlayerInfo,
  PlayerSession,
  ResourceSample,
  RconCommandResponse,
  RustAccessListKind,
  RustConfigUpdateRequest,
  SaveFileEntry,
  SettingsView,
  UptimeScheduleRequest,
  UptimeScheduleView,
  VersionView,
  VRisingAccessListKind,
  WebhookView,
} from './types'

// These have a live push counterpart (see `useLiveSocket`) that keeps their
// cache fresh in near-real-time; the interval here is only a fallback in
// case the WebSocket is down.
const LIVE_FALLBACK_INTERVAL = 30_000

// The backend caches its own GitHub release lookup for hours, so polling
// here is cheap; this just makes sure a long-open dashboard tab notices a
// newly published release without needing a manual reload.
const VERSION_CHECK_INTERVAL = 30 * 60_000

function valheimInstancePath(name: string, suffix = '') {
  return `/games/valheim/instances/${name}${suffix}`
}

export function useVersion() {
  return useQuery({
    queryKey: ['version'],
    queryFn: () => api.get<VersionView>('/version'),
    staleTime: Infinity,
    refetchInterval: VERSION_CHECK_INTERVAL,
  })
}

export function useChangelog() {
  return useQuery({
    queryKey: ['changelog'],
    queryFn: () => api.get<ChangelogRelease[]>('/changelog'),
    staleTime: Infinity,
  })
}

export function useDoctor() {
  return useQuery({
    queryKey: ['doctor'],
    queryFn: () => api.get<CheckResult[]>('/doctor'),
    refetchInterval: 10_000,
  })
}

export function useHostResources() {
  return useQuery({
    queryKey: ['resources', 'host'],
    queryFn: () => api.get<HostResources>('/system/resources'),
    refetchInterval: LIVE_FALLBACK_INTERVAL,
  })
}

export function useHostResourceHistory() {
  return useQuery({
    queryKey: ['resource-history', 'host'],
    queryFn: () => api.get<ResourceSample[]>('/system/resources/history'),
    staleTime: Infinity,
  })
}

export function useInstanceResources(name: string, enabled = true) {
  return useQuery({
    queryKey: ['resources', 'instance', name],
    queryFn: () => api.get<InstanceResources>(valheimInstancePath(name, '/resources')),
    refetchInterval: LIVE_FALLBACK_INTERVAL,
    enabled,
  })
}

// `hours` omitted keeps the existing live-socket-fed, in-memory (~6 minute)
// history — its query key deliberately matches `useLiveSocket`'s writes.
// A specific `hours` reads a downsampled long-range history straight from
// the database instead, under its own query key so it doesn't collide with
// the live one.
export function useInstanceResourceHistory(name: string, hours?: number, enabled = true) {
  return useQuery({
    queryKey: hours
      ? ['resource-history', 'instance', name, hours]
      : ['resource-history', 'instance', name],
    queryFn: () =>
      api.get<ResourceSample[]>(
        valheimInstancePath(name, `/resources/history${hours ? `?hours=${hours}` : ''}`),
      ),
    staleTime: hours ? 60_000 : Infinity,
    enabled,
  })
}

export function usePlayers(name: string, enabled = true) {
  return useQuery({
    queryKey: ['players', name],
    queryFn: () => api.get<PlayerInfo[]>(valheimInstancePath(name, '/players')),
    staleTime: Infinity,
    enabled,
  })
}

export function usePlayerHistory(name: string) {
  return useQuery({
    queryKey: ['players', name, 'history'],
    queryFn: () => api.get<PlayerSession[]>(valheimInstancePath(name, '/players/history')),
  })
}

export function useSaveFiles(game: GameId, name: string, id?: string) {
  return useQuery({
    queryKey: ['save-files', id ?? `${game}/${name}`],
    queryFn: () => api.get<SaveFileEntry[]>(id ? `/instances/${id}/saves` : `/games/${game}/instances/${name}/saves`),
  })
}

// No REST endpoint backs this — unlike `players`, there's nothing worth
// fetching on first load (a fresh page just shows "no save yet" for a few
// seconds until the next live tick arrives). Purely fed by `useLiveSocket`.
export function useLastSaved(name: string) {
  return useQuery({
    queryKey: ['last-saved', name],
    queryFn: () => Promise.resolve<string | null>(null),
    staleTime: Infinity,
  })
}

export function useActivityFeed() {
  return useQuery({
    queryKey: ['activity-feed'],
    queryFn: () => Promise.resolve<ActivityEvent[]>([]),
    initialData: [] as ActivityEvent[],
    staleTime: Infinity,
  })
}

export function useInstances() {
  return useQuery({
    queryKey: ['instances'],
    queryFn: () => api.get<InstanceView[]>('/instances'),
    refetchInterval: LIVE_FALLBACK_INTERVAL,
  })
}

export function useGames() {
  return useQuery({
    queryKey: ['games'],
    queryFn: () => api.get<GameView[]>('/games'),
    staleTime: Infinity,
  })
}

export function useGameInstallStatus(game: GameId) {
  return useQuery({
    queryKey: ['game-install-status', game],
    queryFn: () => api.get<InstallStatusView>(`/games/${game}/install/status`),
    refetchInterval: LIVE_FALLBACK_INTERVAL,
  })
}

export function useInstallGame() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (game: GameId) => api.post<JobHandle>(`/games/${game}/install`),
    onSuccess: (_job, game) => {
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
      queryClient.invalidateQueries({ queryKey: ['game-install-status', game] })
    },
  })
}

export function useManagedInstances() {
  return useQuery({
    queryKey: ['managed-instances'],
    queryFn: () => api.get<ManagedInstanceView[]>('/games/instances'),
    refetchInterval: LIVE_FALLBACK_INTERVAL,
  })
}

export function useManagedInstance(game: GameId, name: string) {
  return useQuery({
    queryKey: ['managed-instances', game, name],
    queryFn: () => api.get<ManagedInstanceView>(`/games/${game}/instances/${name}`),
    refetchInterval: 5_000,
  })
}

export function useManagedInstanceById(id: string) {
  return useQuery({
    queryKey: ['managed-instances', id],
    queryFn: () => api.get<ManagedInstanceView>(`/instances/${id}`),
    enabled: Boolean(id),
    refetchInterval: 5_000,
  })
}

export function useManagedInstanceTransition(id: string, game: GameId, name: string) {
  return useQuery({
    queryKey: ['game-instance-transitions'],
    queryFn: () => Promise.resolve<GameInstanceTransitions>([]),
    initialData: [] as GameInstanceTransitions,
    staleTime: Infinity,
    select: (transitions): InstanceTransition | null => (
      transitions.find((transition) => transition.id === id || (transition.id === null && transition.game === game && transition.name === name))?.transition ?? null
    ),
  })
}

export function useManagedInstanceLogs(game: GameId, name: string, lines = 200, id?: string) {
  return useQuery({
    queryKey: ['managed-instances', id ?? `${game}/${name}`, 'logs', lines],
    queryFn: () => api.get<LogsView>(id ? `/instances/${id}/logs?lines=${lines}` : `/games/${game}/instances/${name}/logs?lines=${lines}`),
    refetchInterval: 5_000,
  })
}

export function usePalworldPlayers(id: string, enabled = true) {
  return useQuery({
    queryKey: ['managed-instances', id, 'palworld', 'players'],
    queryFn: () => api.get<unknown>(`/instances/${id}/palworld/players`),
    refetchInterval: 10_000,
    enabled: Boolean(id) && enabled,
  })
}

export function usePalworldMetrics(id: string, enabled = true) {
  return useQuery({
    queryKey: ['managed-instances', id, 'palworld', 'metrics'],
    queryFn: () => api.get<unknown>(`/instances/${id}/palworld/metrics`),
    refetchInterval: 10_000,
    enabled: Boolean(id) && enabled,
  })
}

export function usePalworldAction(action: 'announce' | 'save' | 'kick' | 'ban' | 'unban' | 'shutdown') {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, request }: { id: string; request?: Record<string, unknown> }) =>
      api.post<unknown>(`/instances/${id}/palworld/${action}`, request),
    onSuccess: (_result, { id }) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'palworld'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id] })
    },
  })
}

export function useManagedResources(id: string, enabled = true) {
  return useQuery({
    queryKey: ['managed-instances', id, 'resources'],
    queryFn: () => api.get<InstanceResources>(`/instances/${id}/resources`),
    refetchInterval: 5_000,
    enabled,
  })
}

export function useManagedResourceHistory(id: string, hours?: number, enabled = true) {
  return useQuery({
    queryKey: ['managed-instances', id, 'resource-history', hours],
    queryFn: () => api.get<ResourceSample[]>(`/instances/${id}/resources/history${hours ? `?hours=${hours}` : ''}`),
    enabled,
  })
}

export function useCreateManagedInstance() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ game, name }: { game: GameId; name: string }) =>
      api.post<ManagedInstanceView>(`/games/${game}/instances`, { name }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['managed-instances'] }),
  })
}

export function useManagedInstanceAction(action: 'start' | 'stop' | 'restart') {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id }: { id: string }) =>
      api.post<ManagedInstanceView>(`/instances/${id}/${action}`),
    onSuccess: (_instance, { id }) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id] })
    },
  })
}

export function useUptimeSchedule(id: string) {
  return useQuery({
    queryKey: ['managed-instances', id, 'uptime-schedule'],
    queryFn: () => api.get<UptimeScheduleView>(`/instances/${id}/uptime-schedule`),
    enabled: Boolean(id),
  })
}

export function useSetUptimeSchedule(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (schedule: UptimeScheduleRequest) =>
      api.put<UptimeScheduleView>(`/instances/${id}/uptime-schedule`, schedule),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'uptime-schedule'] }),
  })
}

export function useDeleteManagedInstance() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, keepBackups }: { id: string; keepBackups?: boolean }) =>
      api.delete<void>(`/instances/${id}${keepBackups ? '?keep_backups=true' : ''}`),
    onSuccess: (_result, { id }) => {
      queryClient.removeQueries({ queryKey: ['managed-instances', id] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
    },
  })
}

export function useUpdateRustConfig() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, request }: { id: string; request: RustConfigUpdateRequest }) =>
      api.put<ManagedInstanceView>(`/instances/${id}/config`, request),
    onSuccess: (instance) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', instance.id] })
    },
  })
}

export function useUpdateGenericConfig() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, request }: { id: string; request: GenericConfigUpdateRequest }) =>
      api.put<ManagedInstanceView>(`/instances/${id}/config`, request),
    onSuccess: (instance) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', instance.id] })
    },
  })
}

export function useExecuteRustRcon() {
  return useMutation({
    mutationFn: ({ id, command }: { id: string; command: string }) =>
      api.post<RconCommandResponse>(`/instances/${id}/rust/rcon`, { command }),
  })
}

export function useExecuteVRisingRcon() {
  return useMutation({
    mutationFn: ({ id, command }: { id: string; command: string }) =>
      api.post<RconCommandResponse>(`/instances/${id}/vrising/rcon`, { command }),
  })
}

export function useWipeRustMap() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, confirmation }: { id: string; confirmation: string }) =>
      api.post<JobHandle>(`/instances/${id}/rust/wipe-map`, { confirmation }),
    onSuccess: (_job, { id }) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instance', id] })
      queryClient.invalidateQueries({ queryKey: ['activity-feed'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useFullWipeRust() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, confirmation }: { id: string; confirmation: string }) =>
      api.post<JobHandle>(`/instances/${id}/rust/full-wipe`, { confirmation }),
    onSuccess: (_job, { id }) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instance', id] })
      queryClient.invalidateQueries({ queryKey: ['activity-feed'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useInstance(name: string) {
  return useQuery({
    queryKey: ['instances', name],
    queryFn: () => api.get<InstanceView>(valheimInstancePath(name, '/status')),
    refetchInterval: 5_000,
  })
}

export function useInstanceTransition(name: string) {
  return useQuery({
    queryKey: ['instance-transitions'],
    queryFn: () => Promise.resolve<InstanceTransitions>({}),
    initialData: {} as InstanceTransitions,
    staleTime: Infinity,
    select: (transitions): InstanceTransition | null => transitions[name] ?? null,
  })
}

export function useInstanceTransitions() {
  return useQuery({
    queryKey: ['instance-transitions'],
    queryFn: () => Promise.resolve<InstanceTransitions>({}),
    initialData: {} as InstanceTransitions,
    staleTime: Infinity,
  })
}

export function useCreateInstance() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api.post<InstanceView>('/instances', { name }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances'] }),
  })
}

export function useCloneInstance(sourceName: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, worldName }: { name: string; worldName: string }) =>
      api.post<InstanceView>(valheimInstancePath(sourceName, '/clone'), { name, world_name: worldName }),
    onSuccess: (instance) => {
      queryClient.invalidateQueries({ queryKey: ['instances'] })
      queryClient.invalidateQueries({ queryKey: ['instances', instance.name] })
      queryClient.invalidateQueries({ queryKey: ['activity-feed'] })
    },
  })
}

function useInstanceAction(action: 'start' | 'stop' | 'restart') {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api.post<void>(valheimInstancePath(name, `/${action}`)),
    onSuccess: (_data, name) => {
      queryClient.invalidateQueries({ queryKey: ['instances'] })
      queryClient.invalidateQueries({ queryKey: ['instances', name] })
      queryClient.invalidateQueries({ queryKey: ['version'] })
    },
  })
}

export const useStartInstance = () => useInstanceAction('start')
export const useStopInstance = () => useInstanceAction('stop')
export const useRestartInstance = () => useInstanceAction('restart')

function useBulkInstanceAction(action: 'start' | 'stop' | 'restart') {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (names: string[]) =>
      api.post<BulkResult[]>(`/instances/bulk/${action}`, { names }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['instances'] })
      queryClient.invalidateQueries({ queryKey: ['version'] })
    },
  })
}

export const useBulkStartInstances = () => useBulkInstanceAction('start')
export const useBulkStopInstances = () => useBulkInstanceAction('stop')
export const useBulkRestartInstances = () => useBulkInstanceAction('restart')

export function useBulkUpdateMods() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (names: string[]) =>
      api.post<JobHandle[]>('/instances/bulk/mods/update', { names }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['jobs'] }),
  })
}

export function useBepInExStatus(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'bepinex-status'],
    queryFn: () => api.get<BepInExStatus>(valheimInstancePath(name, '/bepinex/status')),
  })
}

export function useUpdateBepInEx() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api.post<JobHandle>(valheimInstancePath(name, '/bepinex/update')),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['jobs'] }),
  })
}

export function useBulkUpdateBepInEx() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (names: string[]) =>
      api.post<BulkBepInExResult[]>('/instances/bulk/bepinex/update', { names }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['jobs'] }),
  })
}

export function useRenameInstance() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, newName }: { name: string; newName: string }) =>
      api.post<InstanceView>(valheimInstancePath(name, '/rename'), { new_name: newName }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances'] }),
  })
}

export function useDeleteInstance() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, keepBackups }: { name: string; keepBackups: boolean }) =>
      api.delete<void>(valheimInstancePath(name, `?keep_backups=${keepBackups}`)),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances'] }),
  })
}

export function useConfig(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'config'],
    queryFn: () => api.get<ConfigView>(valheimInstancePath(name, '/config')),
  })
}

export function useUpdateConfig(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: ConfigUpdateRequest) => api.put<ConfigView>(valheimInstancePath(name, '/config'), req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'config'] })
      queryClient.invalidateQueries({ queryKey: ['instances', name] })
    },
  })
}

export function useLogs(name: string, lines = 200) {
  return useQuery({
    queryKey: ['instances', name, 'logs', lines],
    queryFn: () => api.get<LogsView>(valheimInstancePath(name, `/logs?lines=${lines}`)),
  })
}

// Diagnostics for the most recent exit — not part of the live tick (there's
// nothing to push it live for), so this is a plain on-demand fetch.
export function useLastExit(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'last-exit'],
    queryFn: () => api.get<LastExitInfo | null>(valheimInstancePath(name, '/last-exit')),
  })
}

export function useMods(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'mods'],
    queryFn: () => api.get(valheimInstancePath(name, '/mods')) as Promise<InstanceView['installed_mods']>,
  })
}

export function useModSearch(query: string) {
  return useQuery({
    queryKey: ['mods', 'search', query],
    queryFn: () => api.get<ModSearchResult[]>(`/mods/search?q=${encodeURIComponent(query)}`),
    enabled: query.trim().length > 0,
  })
}

// name is passed at mutate-time (rather than bound when the hook is
// created) so the same mutation can be reused across many instances at
// once, e.g. from the global mods page.
export function useAddMod() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, modId }: { name: string; modId: string }) =>
      api.post<JobHandle>(valheimInstancePath(name, '/mods'), { mod_id: modId }),
    onSuccess: (_data, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['mods', 'global'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useRemoveMod() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, modId }: { name: string; modId: string }) =>
      api.delete<void>(valheimInstancePath(name, `/mods/${encodeURIComponent(modId)}`)),
    onSuccess: (_data, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['mods', 'global'] })
    },
  })
}

export function useSetModEnabled() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, modId, enabled }: { name: string; modId: string; enabled: boolean }) =>
      api.post<void>(
        valheimInstancePath(name, `/mods/${encodeURIComponent(modId)}/${enabled ? 'enable' : 'disable'}`),
      ),
    onSuccess: (_data, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['mods', 'global'] })
    },
  })
}

export function useSelectModVersion() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, modId, version }: { name: string; modId: string; version: string }) =>
      api.put<void>(valheimInstancePath(name, `/mods/${encodeURIComponent(modId)}/version`), { version }),
    onSuccess: (_data, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['mods', 'global'] })
    },
  })
}

export function useSetModPinned() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, modId, pinned }: { name: string; modId: string; pinned: boolean }) =>
      api.put<void>(valheimInstancePath(name, `/mods/${encodeURIComponent(modId)}/pinned`), { pinned }),
    onSuccess: (_data, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['mods', 'global'] })
    },
  })
}

export function useUpdateMods() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api.post<JobHandle>(valheimInstancePath(name, '/mods/update')),
    onSuccess: (_data, name) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['mods', 'global'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useBackups(id: string) {
  return useQuery({
    queryKey: ['managed-instances', id, 'backups'],
    refetchInterval: 5_000,
    queryFn: () => api.get<BackupEntry[]>(`/instances/${id}/backups`),
  })
}

export function useCreateBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api.post<JobHandle>(`/instances/${id}/backups/jobs`),
    onSuccess: (_data, id) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'backups'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useRestoreBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, backupId }: { id: string; backupId: string }) =>
      api.post<JobHandle>(`/instances/${id}/backups/${backupId}/restore/job`),
    onSuccess: (_data, { id }) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'backups'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useDeleteBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, backupId }: { id: string; backupId: string }) =>
      api.delete<void>(`/instances/${id}/backups/${backupId}`),
    onSuccess: (_data, { id }) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'backups'] })
    },
  })
}

export function useBackupSchedule(id: string) {
  return useQuery({
    queryKey: ['managed-instances', id, 'backup-schedule'],
    queryFn: () => api.get<BackupScheduleView>(`/instances/${id}/backup-schedule`),
  })
}

export function useSetBackupSchedule(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: Omit<BackupScheduleView, 'last_run_at'>) =>
      api.put<BackupScheduleView>(`/instances/${id}/backup-schedule`, req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'backup-schedule'] })
    },
  })
}

export function useBackupStorage(id: string) {
  return useQuery({
    queryKey: ['managed-instances', id, 'backup-storage'],
    queryFn: () => api.get<BackupStorageView>(`/instances/${id}/backup-storage`),
  })
}

export function useSetBackupStorage(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: BackupStorageRequest) =>
      api.put<BackupStorageView>(`/instances/${id}/backup-storage`, req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'backup-storage'] })
    },
  })
}

export function useGlobalMods() {
  return useQuery({
    queryKey: ['mods', 'global'],
    queryFn: () => api.get<GlobalMod[]>('/mods'),
    refetchInterval: 5_000,
  })
}

export function usePruneMod() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (modId: string) => api.delete<void>(`/mods/${encodeURIComponent(modId)}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['mods', 'global'] }),
  })
}

export function usePruneModVersion() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ modId, version }: { modId: string; version: string }) =>
      api.delete<void>(
        `/mods/${encodeURIComponent(modId)}/versions/${encodeURIComponent(version)}`,
      ),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['mods', 'global'] }),
  })
}

// Nexus Mods has no keyword-search endpoint, so discovery is a one-shot
// "resolve this pasted URL/ID" action (a mutation, unlike `useModSearch`'s
// live-as-you-type query) plus a "trending" list.
export function useNexusLookup() {
  return useMutation({
    mutationFn: (query: string) =>
      api.get<ModSearchResult>(`/mods/nexus/lookup?query=${encodeURIComponent(query)}`),
  })
}

export function useNexusTrending() {
  return useQuery({
    queryKey: ['mods', 'nexus', 'trending'],
    queryFn: () => api.get<ModSearchResult[]>('/mods/nexus/trending'),
    staleTime: 10 * 60_000,
  })
}

export function useUploadMod() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({
      name,
      modName,
      version,
      file,
    }: {
      name: string
      modName: string
      version: string
      file: File
    }) => {
      const formData = new FormData()
      formData.set('name', modName)
      if (version.trim()) formData.set('version', version)
      formData.set('file', file)
      return api.upload<JobHandle>(valheimInstancePath(name, '/mods/upload'), formData)
    },
    onSuccess: (_data, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['mods', 'global'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useSettings() {
  return useQuery({
    queryKey: ['settings'],
    queryFn: () => api.get<SettingsView>('/settings'),
  })
}

export function useSetNexusApiKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (apiKey: string) => api.put<void>('/settings/nexus-api-key', { api_key: apiKey }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['settings'] }),
  })
}

export function useSetInstanceDefaults() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (defaults: InstanceDefaults) =>
      api.put<InstanceDefaults>('/settings/instance-defaults', defaults),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['settings'] }),
  })
}

export function useClearNexusApiKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.delete<void>('/settings/nexus-api-key'),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['settings'] }),
  })
}

export function useList(name: string, kind: ListKind) {
  return useQuery({
    queryKey: ['instances', name, 'lists', kind],
    queryFn: () => api.get<ListView>(valheimInstancePath(name, `/lists/${kind}`)),
  })
}

export function useAddListEntry(name: string, kind: ListKind) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api.post<void>(valheimInstancePath(name, `/lists/${kind}`), { id }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances', name, 'lists', kind] }),
  })
}

export function useRemoveListEntry(name: string, kind: ListKind) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      api.delete<void>(valheimInstancePath(name, `/lists/${kind}/${encodeURIComponent(id)}`)),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances', name, 'lists', kind] }),
  })
}

export function useRustAccessList(id: string, kind: RustAccessListKind) {
  return useQuery({
    queryKey: ['managed-instances', id, 'lists', kind],
    queryFn: () => api.get<ListView>(`/instances/${id}/rust/lists/${kind}`),
  })
}

export function useAddRustAccessListEntry(id: string, kind: RustAccessListKind) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (entryId: string) => api.post<void>(`/instances/${id}/rust/lists/${kind}`, { id: entryId }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'lists', kind] }),
  })
}

export function useRemoveRustAccessListEntry(id: string, kind: RustAccessListKind) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (entryId: string) =>
      api.delete<void>(`/instances/${id}/rust/lists/${kind}/${encodeURIComponent(entryId)}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'lists', kind] }),
  })
}

export function useVRisingAccessList(id: string, kind: VRisingAccessListKind) {
  return useQuery({
    queryKey: ['managed-instances', id, 'lists', kind],
    queryFn: () => api.get<ListView>(`/instances/${id}/vrising/lists/${kind}`),
  })
}

export function useAddVRisingAccessListEntry(id: string, kind: VRisingAccessListKind) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (entryId: string) => api.post<void>(`/instances/${id}/vrising/lists/${kind}`, { id: entryId }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'lists', kind] }),
  })
}

export function useRemoveVRisingAccessListEntry(id: string, kind: VRisingAccessListKind) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (entryId: string) => api.delete<void>(`/instances/${id}/vrising/lists/${kind}/${encodeURIComponent(entryId)}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'lists', kind] }),
  })
}

export function useConfigFiles(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'bepinex-config'],
    queryFn: () => api.get<ConfigFileEntry[]>(valheimInstancePath(name, '/bepinex/config')),
  })
}

export function useConfigFileContent(name: string, filename: string | null) {
  return useQuery({
    queryKey: ['instances', name, 'bepinex-config', filename],
    queryFn: () =>
      api.get<ConfigFileView>(valheimInstancePath(name, `/bepinex/config/${encodeURIComponent(filename!)}`)),
    enabled: filename !== null,
  })
}

export function useSetConfigFileContent(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ filename, content }: { filename: string; content: string }) =>
      api.put<void>(valheimInstancePath(name, `/bepinex/config/${encodeURIComponent(filename)}`), { content }),
    onSuccess: (_data, { filename }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'bepinex-config', filename] })
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'bepinex-config'] })
    },
  })
}

export function useInstallServer() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<JobHandle>('/install'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
      queryClient.invalidateQueries({ queryKey: ['install-status'] })
    },
  })
}

export function useInstallStatus() {
  return useQuery({
    queryKey: ['install-status'],
    queryFn: () => api.get<InstallStatusView>('/install/status'),
    refetchInterval: LIVE_FALLBACK_INTERVAL,
  })
}

export function useJobs() {
  return useQuery({
    queryKey: ['jobs'],
    queryFn: () => api.get<JobSummary[]>('/jobs'),
    refetchInterval: 3_000,
  })
}

export function useWebhooks() {
  return useQuery({
    queryKey: ['webhooks'],
    queryFn: () => api.get<WebhookView[]>('/webhooks'),
  })
}

export function useCreateWebhook() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: { url: string; event_kinds: string[] }) =>
      api.post<WebhookView>('/webhooks', req),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['webhooks'] }),
  })
}

export function useDeleteWebhook() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api.delete<void>(`/webhooks/${id}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['webhooks'] }),
  })
}

export function useUpdateWebhook() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, eventKinds }: { id: string; eventKinds: string[] }) =>
      api.put<void>(`/webhooks/${id}`, { event_kinds: eventKinds }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['webhooks'] }),
  })
}

export function useSetWebhookEnabled() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      api.post<void>(`/webhooks/${id}/${enabled ? 'enable' : 'disable'}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['webhooks'] }),
  })
}

export function useTestWebhook() {
  return useMutation({
    mutationFn: (id: string) => api.post<void>(`/webhooks/${id}/test`),
  })
}
