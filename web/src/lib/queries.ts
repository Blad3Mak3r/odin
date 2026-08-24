import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from './api-client'
import type {
  CheckResult,
  ConfigUpdateRequest,
  ConfigView,
  HostResources,
  InstanceResources,
  InstanceView,
  JobHandle,
  JobSnapshot,
  ListKind,
  ListView,
  LogsView,
  ModSearchResult,
} from './types'

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
    refetchInterval: 3_000,
  })
}

export function useInstanceResources(name: string, enabled = true) {
  return useQuery({
    queryKey: ['resources', 'instance', name],
    queryFn: () => api.get<InstanceResources>(`/instances/${name}/resources`),
    refetchInterval: 3_000,
    enabled,
  })
}

export function useInstances() {
  return useQuery({
    queryKey: ['instances'],
    queryFn: () => api.get<InstanceView[]>('/instances'),
    refetchInterval: 5_000,
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
    refetchInterval: 4_000,
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

export function useAddMod(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (modId: string) => api.post<JobHandle>(`/instances/${name}/mods`, { mod_id: modId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
  })
}

export function useRemoveMod(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (modId: string) => api.delete<void>(`/instances/${name}/mods/${modId}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] }),
  })
}

export function useSetModEnabled(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ modId, enabled }: { modId: string; enabled: boolean }) =>
      api.post<void>(`/instances/${name}/mods/${modId}/${enabled ? 'enable' : 'disable'}`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] }),
  })
}

export function useUpdateMods(name: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<JobHandle>(`/instances/${name}/mods/update`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['instances', name, 'mods'] })
      queryClient.invalidateQueries({ queryKey: ['jobs'] })
    },
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

export function useInstallServer() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.post<JobHandle>('/install'),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['jobs'] }),
  })
}

export function useJobs() {
  return useQuery({
    queryKey: ['jobs'],
    queryFn: () => api.get<JobSnapshot[]>('/jobs'),
    refetchInterval: 3_000,
  })
}
