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
  GameView,
  HostResources,
  InstallStatusView,
  InstanceResources,
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
  ResourceSample,
  RustConfigUpdateRequest,
  SettingsView,
  VersionView,
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
    queryFn: () => api.get<InstanceResources>(`/instances/${name}/resources`),
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
        `/instances/${name}/resources/history${hours ? `?hours=${hours}` : ''}`,
      ),
    staleTime: hours ? 60_000 : Infinity,
    enabled,
  })
}

export function usePlayers(name: string, enabled = true) {
  return useQuery({
    queryKey: ['players', name],
    queryFn: () => api.get<PlayerInfo[]>(`/instances/${name}/players`),
    staleTime: Infinity,
    enabled,
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

export function useManagedInstanceLogs(game: GameId, name: string) {
  return useQuery({
    queryKey: ['managed-instances', game, name, 'logs'],
    queryFn: () => api.get<LogsView>(`/games/${game}/instances/${name}/logs`),
    refetchInterval: 5_000,
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
    mutationFn: ({ game, name }: { game: GameId; name: string }) =>
      api.post<ManagedInstanceView>(`/games/${game}/instances/${name}/${action}`),
    onSuccess: (_instance, variables) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', variables.game, variables.name] })
    },
  })
}

export function useUpdateRustConfig() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, request }: { name: string; request: RustConfigUpdateRequest }) =>
      api.put<ManagedInstanceView>(`/games/rust/instances/${name}/config`, request),
    onSuccess: (_instance, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', 'rust', name] })
    },
  })
}

export function useManagedBackups(game: GameId, name: string) {
  return useQuery({
    queryKey: ['managed-instances', game, name, 'backups'],
    queryFn: () => api.get<BackupEntry[]>(`/games/${game}/instances/${name}/backups`),
  })
}

export function useCreateManagedBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ game, name }: { game: GameId; name: string }) =>
      api.post<BackupEntry>(`/games/${game}/instances/${name}/backups`),
    onSuccess: (_backup, { game, name }) =>
      queryClient.invalidateQueries({ queryKey: ['managed-instances', game, name, 'backups'] }),
  })
}

export function useRestoreManagedBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ game, name, backupId }: { game: GameId; name: string; backupId: string }) =>
      api.post<void>(`/games/${game}/instances/${name}/backups/${backupId}/restore`),
    onSuccess: (_result, { game, name }) =>
      queryClient.invalidateQueries({ queryKey: ['managed-instances', game, name, 'backups'] }),
  })
}

export function useInstance(name: string) {
  return useQuery({
    queryKey: ['instances', name],
    queryFn: () => api.get<InstanceView>(`/instances/${name}`),
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
      api.post<InstanceView>(`/instances/${sourceName}/clone`, { name, world_name: worldName }),
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
    mutationFn: (name: string) => api.post<InstanceView>(`/instances/${name}/${action}`),
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
    queryFn: () => api.get<BepInExStatus>(`/instances/${name}/bepinex/status`),
  })
}

export function useUpdateBepInEx() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api.post<JobHandle>(`/instances/${name}/bepinex/update`),
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
      api.post<InstanceView>(`/instances/${name}/rename`, { new_name: newName }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances'] }),
  })
}

export function useDeleteInstance() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, keepBackups }: { name: string; keepBackups: boolean }) =>
      api.delete<void>(`/instances/${name}?keep_backups=${keepBackups}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances'] }),
  })
}

export function useConfig(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'config'],
    queryFn: () => api.get<ConfigView>(`/instances/${name}/config`),
  })
}

export function useUpdateConfig(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: ConfigUpdateRequest) => api.put<ConfigView>(`/instances/${name}/config`, req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'config'] })
      queryClient.invalidateQueries({ queryKey: ['instances', name] })
    },
  })
}

export function useLogs(name: string, lines = 200) {
  return useQuery({
    queryKey: ['instances', name, 'logs', lines],
    queryFn: () => api.get<LogsView>(`/instances/${name}/logs?lines=${lines}`),
  })
}

// Diagnostics for the most recent exit — not part of the live tick (there's
// nothing to push it live for), so this is a plain on-demand fetch.
export function useLastExit(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'last-exit'],
    queryFn: () => api.get<LastExitInfo | null>(`/instances/${name}/last-exit`),
  })
}

export function useMods(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'mods'],
    queryFn: () => api.get(`/instances/${name}/mods`) as Promise<InstanceView['installed_mods']>,
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
      api.post<JobHandle>(`/instances/${name}/mods`, { mod_id: modId }),
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
      api.delete<void>(`/instances/${name}/mods/${encodeURIComponent(modId)}`),
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
        `/instances/${name}/mods/${encodeURIComponent(modId)}/${enabled ? 'enable' : 'disable'}`,
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
      api.put<void>(`/instances/${name}/mods/${encodeURIComponent(modId)}/version`, { version }),
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
      api.put<void>(`/instances/${name}/mods/${encodeURIComponent(modId)}/pinned`, { pinned }),
    onSuccess: (_data, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['mods', 'global'] })
    },
  })
}

export function useUpdateMods() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api.post<JobHandle>(`/instances/${name}/mods/update`),
    onSuccess: (_data, name) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['mods', 'global'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useBackups(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'backups'],
    queryFn: () => api.get<BackupEntry[]>(`/instances/${name}/backups`),
  })
}

export function useCreateBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api.post<JobHandle>(`/instances/${name}/backups`),
    onSuccess: (_data, name) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'backups'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useRestoreBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, backupId }: { name: string; backupId: string }) =>
      api.post<JobHandle>(`/instances/${name}/backups/${backupId}/restore`),
    onSuccess: (_data, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'backups'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useDeleteBackup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, backupId }: { name: string; backupId: string }) =>
      api.delete<void>(`/instances/${name}/backups/${backupId}`),
    onSuccess: (_data, { name }) => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'backups'] })
    },
  })
}

export function useBackupSchedule(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'backup-schedule'],
    queryFn: () => api.get<BackupScheduleView>(`/instances/${name}/backup-schedule`),
  })
}

export function useSetBackupSchedule(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: Omit<BackupScheduleView, 'last_run_at'>) =>
      api.put<BackupScheduleView>(`/instances/${name}/backup-schedule`, req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'backup-schedule'] })
    },
  })
}

export function useBackupStorage(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'backup-storage'],
    queryFn: () => api.get<BackupStorageView>(`/instances/${name}/backup-storage`),
  })
}

export function useSetBackupStorage(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: BackupStorageRequest) =>
      api.put<BackupStorageView>(`/instances/${name}/backup-storage`, req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'backup-storage'] })
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
      return api.upload<JobHandle>(`/instances/${name}/mods/upload`, formData)
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
    queryFn: () => api.get<ListView>(`/instances/${name}/lists/${kind}`),
  })
}

export function useAddListEntry(name: string, kind: ListKind) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api.post<void>(`/instances/${name}/lists/${kind}`, { id }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances', name, 'lists', kind] }),
  })
}

export function useRemoveListEntry(name: string, kind: ListKind) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      api.delete<void>(`/instances/${name}/lists/${kind}/${encodeURIComponent(id)}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances', name, 'lists', kind] }),
  })
}

export function useConfigFiles(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'bepinex-config'],
    queryFn: () => api.get<ConfigFileEntry[]>(`/instances/${name}/bepinex/config`),
  })
}

export function useConfigFileContent(name: string, filename: string | null) {
  return useQuery({
    queryKey: ['instances', name, 'bepinex-config', filename],
    queryFn: () =>
      api.get<ConfigFileView>(`/instances/${name}/bepinex/config/${encodeURIComponent(filename!)}`),
    enabled: filename !== null,
  })
}

export function useSetConfigFileContent(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ filename, content }: { filename: string; content: string }) =>
      api.put<void>(`/instances/${name}/bepinex/config/${encodeURIComponent(filename)}`, { content }),
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
