import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from './api-client'
import type {
  ActivityEvent,
  BackupEntry,
  CheckResult,
  ConfigFileEntry,
  ConfigFileView,
  ConfigUpdateRequest,
  ConfigView,
  GlobalMod,
  HostResources,
  InstallStatusView,
  InstanceResources,
  InstanceView,
  JobHandle,
  JobSummary,
  ListKind,
  ListView,
  LogsView,
  ModSearchResult,
  PlayerInfo,
  ResourceSample,
  SettingsView,
  VersionView,
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

export function useInstanceResourceHistory(name: string, enabled = true) {
  return useQuery({
    queryKey: ['resource-history', 'instance', name],
    queryFn: () => api.get<ResourceSample[]>(`/instances/${name}/resources/history`),
    staleTime: Infinity,
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

export function useInstance(name: string) {
  return useQuery({
    queryKey: ['instances', name],
    queryFn: () => api.get<InstanceView>(`/instances/${name}`),
    refetchInterval: 5_000,
  })
}

export function useCreateInstance() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api.post<InstanceView>('/instances', { name }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances'] }),
  })
}

function useInstanceAction(action: 'start' | 'stop' | 'restart') {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api.post<InstanceView>(`/instances/${name}/${action}`),
    onSuccess: (_data, name) => {
      queryClient.invalidateQueries({ queryKey: ['instances'] })
      queryClient.invalidateQueries({ queryKey: ['instances', name] })
    },
  })
}

export const useStartInstance = () => useInstanceAction('start')
export const useStopInstance = () => useInstanceAction('stop')
export const useRestartInstance = () => useInstanceAction('restart')

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

export function useSetList(name: string, kind: ListKind) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (ids: string[]) => api.put<void>(`/instances/${name}/lists/${kind}`, { ids }),
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
