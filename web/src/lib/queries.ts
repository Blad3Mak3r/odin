import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { api, setCsrfToken } from './api-client'
import type {
  ActivityEvent,
  AuditEvent,
  AuthSession,
  AuthStatus,
  AuthUser,
  UserRole,
  ArkCluster,
  AdvancedConfigChange,
  AdvancedConfigView,
  BackupEntry,
  BackupScheduleView,
  BackupStorageRequest,
  BackupStorageView,
  BepInExStatus,
  BulkBepInExResult,
  ChangelogRelease,
  UpdateCheckView,
  CheckResult,
  ConfigFileEntry,
  ConfigFileView,
  ConfigUpdateRequest,
  ConfigView,
  CurseForgeFeatured,
  CurseForgeMod,
  CurseForgeSearchPage,
  GlobalMod,
  GameId,
  GameInstanceTransitions,
  GameView,
  Cs2LaunchOptions,
  GenericConfigUpdateRequest,
  HostResources,
  InstanceResources,
  InstanceDefaults,
  InstanceTransition,
  InstanceTransitions,
  InstanceView,
  ManagedInstanceView,
  JobHandle,
  JobSummary,
  ListKind,
  ListView,
  LogFilesView,
  LogsView,
  ModSearchResult,
  PlayerInfo,
  PlayerSession,
  ResourceSample,
  RconCommandResponse,
  ResourceLimits,
  RuntimeImageRole,
  RuntimeImageView,
  RawAdvancedConfigFile,
  MoveToSharedInstallResult,
  SharedInstallView,
  SevenDaysTemplateReview,
  RustAccessListKind,
  RustConfigUpdateRequest,
  SaveFileEntry,
  SevenDaysPlayer,
  SettingsView,
  RuntimeHealth,
  StartupReport,
  UptimeScheduleRequest,
  UptimeScheduleView,
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

function valheimInstancePath(id: string, suffix = '') {
  return `/instances/${id}/valheim${suffix}`
}

export function useVersion() {
  return useQuery({
    queryKey: ['version'],
    queryFn: () => api.get<VersionView>('/version'),
    staleTime: Infinity,
    refetchInterval: VERSION_CHECK_INTERVAL,
  })
}

export function useSystemUpdate() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<void>('/update'),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['version'] }),
  })
}

export function useChangelog() {
  return useQuery({
    queryKey: ['changelog'],
    queryFn: () => api.get<ChangelogRelease[]>('/changelog'),
    staleTime: Infinity,
  })
}

export function useUpdateCheckStatus() {
  return useQuery({
    queryKey: ['changelog', 'update-check'],
    queryFn: () => api.get<UpdateCheckView>('/changelog/update-check'),
    refetchInterval: 1_000,
  })
}

export function useCheckForUpdates() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<UpdateCheckView>('/changelog/update-check'),
    onSuccess: (result) => queryClient.setQueryData(['changelog', 'update-check'], result),
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

export function useInstanceResources(id: string, enabled = true) {
  return useQuery({
    queryKey: ['resources', 'instance', id],
    queryFn: () => api.get<InstanceResources>(`/instances/${id}/resources`),
    refetchInterval: LIVE_FALLBACK_INTERVAL,
    enabled,
  })
}

// `hours` omitted keeps the existing live-socket-fed, in-memory (~6 minute)
// history — its query key deliberately matches `useLiveSocket`'s writes.
// A specific `hours` reads a downsampled long-range history straight from
// the database instead, under its own query key so it doesn't collide with
// the live one.
export function useInstanceResourceHistory(id: string, hours?: number, enabled = true) {
  return useQuery({
    queryKey: hours
      ? ['resource-history', 'instance', id, hours]
      : ['resource-history', 'instance', id],
    queryFn: () =>
      api.get<ResourceSample[]>(
        `/instances/${id}/resources/history${hours ? `?hours=${hours}` : ''}`,
      ),
    staleTime: hours ? 60_000 : Infinity,
    enabled,
  })
}

export function usePlayers(id: string, enabled = true) {
  return useQuery({
    queryKey: ['players', id],
    queryFn: () => api.get<PlayerInfo[]>(valheimInstancePath(id, '/players')),
    staleTime: Infinity,
    enabled,
  })
}

export function usePlayerHistory(id: string) {
  return useQuery({
    queryKey: ['players', id, 'history'],
    queryFn: () => api.get<PlayerSession[]>(valheimInstancePath(id, '/players/history')),
  })
}

export type SaveFileSource =
  | { kind: 'instance'; id: string }
  | { kind: 'ark-cluster'; id: string }

const savesPath = (source: SaveFileSource, path = '') => {
  const root = source.kind === 'instance'
    ? `/instances/${encodeURIComponent(source.id)}/saves`
    : `/ark-clusters/${encodeURIComponent(source.id)}/files`
  return `${root}${path ? `/${path.split('/').map(encodeURIComponent).join('/')}` : ''}`
}

export const saveFileDownloadUrl = (source: SaveFileSource, path: string) =>
  `/api${savesPath(source, path)}`

/** Lists one folder of a save source; `path` is empty for the root. */
export function useSaveFiles(source: SaveFileSource, path: string) {
  return useQuery({
    queryKey: ['save-files', source.kind, source.id, path],
    queryFn: () => api.get<SaveFileEntry[]>(`${savesPath(source)}?path=${encodeURIComponent(path)}`),
    // A missing folder is a 404 the user fixes by navigating up, so don't retry it.
    retry: false,
    // Keep the previous folder on screen while the next one loads so the page height doesn't collapse.
    placeholderData: keepPreviousData,
  })
}

export function useUploadSaveFiles() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ source, path, files }: { source: SaveFileSource; path: string; files: File[] }) => {
      const formData = new FormData()
      for (const file of files) formData.append('file', file)
      return api.upload<void>(`${savesPath(source)}?path=${encodeURIComponent(path)}`, formData)
    },
    onSuccess: (_data, { source }) => queryClient.invalidateQueries({ queryKey: ['save-files', source.kind, source.id] }),
  })
}

export function useDeleteSaveFile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ source, path }: { source: SaveFileSource; path: string }) => api.delete<void>(savesPath(source, path)),
    onSuccess: (_data, { source }) => queryClient.invalidateQueries({ queryKey: ['save-files', source.kind, source.id] }),
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
    // The API lists games in driver order; every view shows them by name.
    queryFn: async () => (await api.get<GameView[]>('/games')).sort((a, b) => a.name.localeCompare(b.name)),
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

/** `date` selects an archived day (`YYYY-MM-DD`); omit it for today's log. */
export function useManagedInstanceLogs(id: string, lines = 200, date?: string) {
  return useQuery({
    queryKey: ['managed-instances', id, 'logs', lines, date ?? 'current'],
    queryFn: () => api.get<LogsView>(`/instances/${id}/logs?lines=${lines}${date ? `&date=${date}` : ''}`),
    // An archived day no longer changes.
    refetchInterval: date ? false : 5_000,
    enabled: Boolean(id),
  })
}

export function useManagedInstanceLogFiles(id: string) {
  return useQuery({
    queryKey: ['managed-instances', id, 'log-files'],
    queryFn: () => api.get<LogFilesView>(`/instances/${id}/logs/files`),
    refetchInterval: 60_000,
    enabled: Boolean(id),
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
    mutationFn: ({ game, name, install_mode, ark_launch, cs2_launch }: { game: GameId; name: string; install_mode?: 'isolated' | 'shared'; ark_launch?: { map: string; session_name: string; mods: number[] }; cs2_launch?: Cs2LaunchOptions }) =>
      api.post<ManagedInstanceView>(`/games/${game}/instances`, { name, install_mode, ark_launch, cs2_launch }),
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
    // A failed start is recorded server-side; refresh it so the Startup tab explains why.
    onError: (_error, { id }) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'startup'] })
    },
  })
}


export function useUpdateRuntime() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api.post<JobHandle>(`/instances/${id}/runtime/update`),
    onSuccess: (_job, id) => {
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id] })
    },
  })
}

export function useRuntimeHealth(id: string, enabled = true) {
  return useQuery({
    queryKey: ['managed-instances', id, 'runtime-health'],
    queryFn: () => api.get<RuntimeHealth>(`/instances/${id}/runtime/health`),
    enabled: enabled && Boolean(id),
  })
}

/** Polls while a start is in flight so the Startup tab shows each phase as it happens. */
export function useStartupReport(id: string) {
  return useQuery({
    queryKey: ['managed-instances', id, 'startup'],
    queryFn: () => api.get<StartupReport>(`/instances/${id}/startup`),
    enabled: Boolean(id),
    refetchInterval: (query) => (query.state.data?.attempts[0]?.outcome === 'running' ? 1500 : false),
  })
}

export function useRepairRuntime() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api.post<void>(`/instances/${id}/runtime/repair`),
    onSuccess: (_result, id) => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'runtime-health'] })
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
      queryClient.invalidateQueries({ queryKey: ['managed-instances', instance.id, 'advanced-config'] })
    },
  })
}

export function useAdvancedConfig(id: string, enabled: boolean) {
  return useQuery({
    queryKey: ['managed-instances', id, 'advanced-config'],
    queryFn: () => api.get<AdvancedConfigView>(`/instances/${id}/config/advanced`),
    enabled,
  })
}

export function useSharedAdvancedConfig(game: GameId, enabled: boolean) {
  return useQuery({ queryKey: ['games', game, 'advanced-config'], queryFn: () => api.get<AdvancedConfigView>(`/games/${game}/config/advanced`), enabled })
}

export function useUpdateSharedAdvancedConfig(game: GameId) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (changes: AdvancedConfigChange[]) => api.put<void>(`/games/${game}/config/advanced`, { changes }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['games', game, 'advanced-config'] }),
  })
}

export function useSharedRawAdvancedConfig(game: GameId, file: string, enabled: boolean) {
  return useQuery({ queryKey: ['games', game, 'advanced-config', file, 'code'], queryFn: () => api.get<RawAdvancedConfigFile>(`/games/${game}/config/files/${file}`), enabled })
}

export function useUpdateSharedRawAdvancedConfig(game: GameId, file: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (content: string) => api.put<void>(`/games/${game}/config/files/${file}`, { content }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['games', game, 'advanced-config'] })
      queryClient.invalidateQueries({ queryKey: ['games', game, 'advanced-config', file, 'code'] })
    },
  })
}

export function useRawAdvancedConfig(id: string, file: string, enabled: boolean) {
  return useQuery({ queryKey: ['managed-instances', id, 'advanced-config', file, 'code'], queryFn: () => api.get<RawAdvancedConfigFile>(`/instances/${id}/config/files/${file}`), enabled })
}

export function useUpdateRawAdvancedConfig(id: string, file: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (content: string) => api.put<void>(`/instances/${id}/config/files/${file}`, { content }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'advanced-config'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'advanced-config', file, 'code'] })
    },
  })
}

export function useSharedInstall(game: GameId, enabled: boolean) {
  return useQuery({
    queryKey: ['games', game, 'install'],
    queryFn: () => api.get<SharedInstallView>(`/games/${game}/install`),
    enabled,
    refetchInterval: (query) => query.state.data?.running ? 2_000 : false,
  })
}

export function useArkClusters() {
  return useQuery({ queryKey: ['ark-clusters'], queryFn: () => api.get<ArkCluster[]>('/ark-clusters') })
}

export function useCreateArkCluster() {
  const queryClient = useQueryClient()
  return useMutation({ mutationFn: (input: { name: string; cluster_id: string }) => api.post<ArkCluster>('/ark-clusters', input), onSuccess: () => queryClient.invalidateQueries({ queryKey: ['ark-clusters'] }) })
}

export function useSetArkClusterMembers() {
  const queryClient = useQueryClient()
  return useMutation({ mutationFn: ({ id, members }: { id: string; members: string[] }) => api.put<void>(`/ark-clusters/${id}/members`, { members }), onSuccess: () => queryClient.invalidateQueries({ queryKey: ['ark-clusters'] }) })
}

export function useDeleteArkCluster() {
  const queryClient = useQueryClient()
  return useMutation({ mutationFn: (id: string) => api.delete<void>(`/ark-clusters/${id}`), onSuccess: () => queryClient.invalidateQueries({ queryKey: ['ark-clusters'] }) })
}

export function useInstallShared(game: GameId) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<JobHandle>(`/games/${game}/install`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['games', game, 'install'] }),
  })
}

export function useMoveToSharedInstall() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, delete_private_files }: { id: string; delete_private_files: boolean }) =>
      api.post<MoveToSharedInstallResult>(`/instances/${id}/shared-install`, { delete_private_files }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['games'] })
    },
  })
}

export function useUpdateAdvancedConfig(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (changes: AdvancedConfigChange[]) =>
      api.put<void>(`/instances/${id}/config/advanced`, { changes }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'advanced-config'] }),
  })
}

export function useInitializeAdvancedConfig(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<void>(`/instances/${id}/config/advanced/init`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'advanced-config'] }),
  })
}

export function useSevenDaysTemplateReview(id: string, enabled: boolean) {
  return useQuery({
    queryKey: ['managed-instances', id, '7d2d', 'config-template'],
    queryFn: () => api.get<SevenDaysTemplateReview>(`/instances/${id}/7d2d/config-template`),
    enabled,
  })
}

export function useApplySevenDaysTemplateReview(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (keys: string[]) => api.post<void>(`/instances/${id}/7d2d/config-template`, { keys }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, '7d2d', 'config-template'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'advanced-config'] })
    },
  })
}

export function useAdoptSevenDaysTemplateBaseline(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<void>(`/instances/${id}/7d2d/config-template/adopt`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['managed-instances', id, '7d2d', 'config-template'] }),
  })
}

export function useResourceLimits(id: string) {
  return useQuery({
    queryKey: ['managed-instances', id, 'resource-limits'],
    queryFn: () => api.get<ResourceLimits>(`/instances/${id}/resource-limits`),
    enabled: Boolean(id),
  })
}

export function useSetResourceLimits(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (limits: ResourceLimits) => api.put<ResourceLimits>(`/instances/${id}/resource-limits`, limits),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'resource-limits'] })
    },
  })
}

export function useUpdateValheimConfig() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, request }: { id: string; request: ConfigUpdateRequest }) =>
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
    mutationFn: (id: string) => api.post<JobHandle>(valheimInstancePath(id, '/bepinex/update')),
    onSuccess: (_job, id) => {
      queryClient.invalidateQueries({ queryKey: ['instances', id, 'bepinex-status'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useInstallBepInEx() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api.post<JobHandle>(valheimInstancePath(id, '/bepinex/install')),
    onSuccess: (_job, id) => {
      queryClient.invalidateQueries({ queryKey: ['instances', id, 'bepinex-status'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
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

export function useConfig(id: string) {
  return useQuery({
    queryKey: ['managed-instances', id, 'config'],
    queryFn: () => api.get<ConfigView>(`/instances/${id}/config`),
  })
}

export function useUpdateConfig(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: ConfigUpdateRequest) => api.put<ManagedInstanceView>(`/instances/${id}/config`, req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id, 'config'] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances', id] })
      queryClient.invalidateQueries({ queryKey: ['managed-instances'] })
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

export function useMods(name: string) {
  return useQuery({
    queryKey: ['instances', name, 'mods'],
    queryFn: () => api.get(valheimInstancePath(name, '/mods')) as Promise<InstanceView['installed_mods']>,
  })
}

export type SevenDaysModInfo = { name: string; display_name: string; version: string; description: string | null; author: string | null; website: string | null }
export function useSevenDaysMods(id: string) {
  return useQuery({ queryKey: ['managed-instances', id, '7d2d', 'mods'], queryFn: () => api.get<SevenDaysModInfo[]>(`/instances/${id}/7d2d/mods`) })
}
export function useUploadSevenDaysMod() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, file, replace }: { id: string; file: File; replace: boolean }) => {
      const form = new FormData(); form.set('file', file); if (replace) form.set('replace', 'true')
      return api.upload<JobHandle>(`/instances/${id}/7d2d/mods/upload`, form)
    },
    onSuccess: (_result, { id }) => { queryClient.invalidateQueries({ queryKey: ['managed-instances', id, '7d2d', 'mods'] }); queryClient.invalidateQueries({ queryKey: ['jobs'] }) },
  })
}
export function useRemoveSevenDaysMod() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, modName }: { id: string; modName: string }) =>
      api.delete<void>(`/instances/${id}/7d2d/mods/${encodeURIComponent(modName)}`),
    onSuccess: (_result, { id }) => queryClient.invalidateQueries({ queryKey: ['managed-instances', id, '7d2d', 'mods'] }),
  })
}

export function useExecuteSevenDaysConsole() {
  return useMutation({
    mutationFn: ({ id, command }: { id: string; command: string }) => api.post<RconCommandResponse>(`/instances/${id}/7d2d/console`, { command }),
  })
}

export function useSevenDaysPlayers(id: string, enabled: boolean) {
  return useQuery({ queryKey: ['managed-instances', id, '7d2d', 'players'], queryFn: () => api.get<SevenDaysPlayer[]>(`/instances/${id}/7d2d/players`), enabled, refetchInterval: enabled ? 15_000 : false })
}

export function useSevenDaysPlayerHistory(id: string, enabled: boolean) {
  return useQuery({ queryKey: ['managed-instances', id, '7d2d', 'players', 'history'], queryFn: () => api.get<PlayerSession[]>(`/instances/${id}/7d2d/players/history`), enabled, refetchInterval: enabled ? 15_000 : false })
}

export function useSevenDaysPlayerAction(id: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ player, action, reason }: { player: string; action: 'kick' | 'ban' | 'unban' | 'admin' | 'whitelist' | 'remove-admin' | 'remove-whitelist'; reason?: string }) => {
      const path = action === 'remove-admin' ? 'admin/remove' : action === 'remove-whitelist' ? 'whitelist/remove' : action
      return api.post<void>(`/instances/${id}/7d2d/players/${encodeURIComponent(player)}/${path}`, reason === undefined ? undefined : { reason })
    },
    onSuccess: () => { queryClient.invalidateQueries({ queryKey: ['managed-instances', id, '7d2d', 'players'] }); queryClient.invalidateQueries({ queryKey: ['managed-instances', id, '7d2d', 'players', 'history'] }) },
  })
}

export function useSevenDaysAccessList(id: string) {
  return useMutation({
    mutationFn: (kind: 'bans' | 'admins' | 'whitelist') => api.get<RconCommandResponse>(`/instances/${id}/7d2d/access/${kind}`),
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
    mutationFn: (id: string) => api.post<JobHandle>(valheimInstancePath(id, '/mods/update')),
    onSuccess: (_data, id) => {
      queryClient.invalidateQueries({ queryKey: ['instances', id, 'mods'] })
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

// CurseForge powers the ARK: Survival Ascended mod picker. The API key is
// operator configuration, so `status` tells the UI whether to offer it at all.
export function useCurseForgeStatus() {
  return useQuery({
    queryKey: ['curseforge', 'status'],
    queryFn: () => api.get<{ configured: boolean }>('/mods/curseforge/status'),
    staleTime: 30_000,
  })
}

export function useCurseForgeMods(ids: number[]) {
  return useQuery({
    queryKey: ['curseforge', 'mods', ids],
    queryFn: () => api.post<CurseForgeMod[]>('/mods/curseforge/resolve', { ids }),
    enabled: ids.length > 0,
    placeholderData: keepPreviousData,
    staleTime: 10 * 60_000,
  })
}

export function useCurseForgeFeatured() {
  return useQuery({
    queryKey: ['curseforge', 'featured'],
    queryFn: () => api.get<CurseForgeFeatured>('/mods/curseforge/featured'),
    staleTime: 10 * 60_000,
  })
}

export function useCurseForgeSearch(query: string) {
  return useInfiniteQuery({
    queryKey: ['curseforge', 'search', query],
    queryFn: ({ pageParam }) =>
      api.get<CurseForgeSearchPage>(
        `/mods/curseforge/search?q=${encodeURIComponent(query)}&index=${pageParam}`,
      ),
    initialPageParam: 0,
    getNextPageParam: (last, pages) => {
      const loaded = pages.reduce((count, page) => count + page.mods.length, 0)
      return last.mods.length > 0 && loaded < last.total ? loaded : undefined
    },
    enabled: query.trim().length > 0,
    staleTime: 5 * 60_000,
  })
}

// A pasted numeric id or mod page URL, resolved one-shot like `useNexusLookup`.
export function useCurseForgeLookup() {
  return useMutation({
    mutationFn: (query: string) =>
      api.get<CurseForgeMod>(`/mods/curseforge/lookup?query=${encodeURIComponent(query)}`),
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

export function useSettings(live = false) {
  return useQuery({
    queryKey: ['settings'],
    queryFn: () => api.get<SettingsView>('/settings'),
    refetchInterval: live ? 10_000 : false,
  })
}

export function useRuntimeImages() {
  return useQuery({
    queryKey: ['settings', 'runtime-images'],
    queryFn: () => api.get<RuntimeImageView[]>('/settings/runtime-images'),
  })
}

export function useCheckRuntimeImages() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<void>('/settings/runtime-images/check'),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['settings', 'runtime-images'] }),
  })
}

export function usePullRuntimeImage() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (role: RuntimeImageRole) => api.post<JobHandle>(`/settings/runtime-images/${role}/pull`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['settings', 'runtime-images'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
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

export function useAuthStatus() {
  return useQuery({
    queryKey: ['auth', 'status'],
    queryFn: async () => {
      const status = await api.get<AuthStatus>('/auth/status')
      setCsrfToken(status.csrf_token)
      return status
    },
    retry: false,
  })
}

/** Drops cached server data and re-reads who is signed in. */
function useResetSession() {
  const queryClient = useQueryClient()
  return () => {
    queryClient.removeQueries({ predicate: (query) => query.queryKey[0] !== 'auth' })
    return queryClient.invalidateQueries({ queryKey: ['auth', 'status'] })
  }
}

export type AuthRequest =
  | { login: string; password: string; remember: boolean }
  | { token: string; email: string; username?: string; password: string }
  | { token: string; email: string; password: string }

export function useAuthenticate(mode: 'login' | 'setup' | 'recover') {
  const reset = useResetSession()
  return useMutation({
    mutationFn: (req: AuthRequest) => api.post<AuthUser>(`/auth/${mode}`, req),
    onSuccess: reset,
  })
}

export function useLogout() {
  const reset = useResetSession()
  return useMutation({
    mutationFn: () => api.post<void>('/auth/logout'),
    onSettled: reset,
  })
}

export function useChangePassword() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: { current_password: string; new_password: string }) =>
      api.post<void>('/auth/password', req),
    // The server starts a fresh session, which carries a new CSRF token.
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['auth', 'status'] }),
  })
}

export function useUsers() {
  return useQuery({
    queryKey: ['users'],
    queryFn: () => api.get<AuthUser[]>('/users'),
  })
}

export function useCreateUser() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: {
      email: string
      username?: string
      password: string
      role: UserRole
      require_change: boolean
    }) =>
      api.post<AuthUser>('/users', req),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['users'] }),
  })
}

export function useUpdateUser() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({
      email,
      ...req
    }: {
      email: string
      role?: UserRole
      password?: string
      require_change?: boolean
    }) =>
      api.put<void>(`/users/${encodeURIComponent(email)}`, req),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['users'] }),
  })
}

export function useDeleteUser() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (email: string) => api.delete<void>(`/users/${encodeURIComponent(email)}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['users'] }),
  })
}

export function useSessions() {
  return useQuery({
    queryKey: ['auth', 'sessions'],
    queryFn: () => api.get<AuthSession[]>('/auth/sessions'),
  })
}

export function useRevokeSession() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api.delete<void>(`/auth/sessions/${id}`),
    // Ending the current session signs out; the status refetch shows the login.
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['auth'] }),
  })
}

export function useRevokeOtherSessions() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<void>('/auth/sessions/revoke-others'),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['auth', 'sessions'] }),
  })
}

export function useAuditLog() {
  return useQuery({
    queryKey: ['auth', 'audit'],
    queryFn: () => api.get<AuditEvent[]>('/auth/audit?limit=100'),
  })
}
