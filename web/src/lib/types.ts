export interface CheckResult {
  label: string
  ok: boolean
  critical: boolean
  detail: string | null
}

export interface InstalledMod {
  mod_id: string
  version: string
  installed_at: string
  enabled: boolean
}

export interface InstanceView {
  name: string
  port: number
  world_name: string
  password: string | null
  public: boolean
  created_at: string
  last_started_at: string | null
  last_stopped_at: string | null
  tmux_session: string
  bepinex_installed: boolean
  installed_mods: InstalledMod[]
  running: boolean
}

export interface ConfigView {
  world_name: string
  port: number
  password: string | null
  public: boolean
}

export interface ConfigUpdateRequest {
  world?: string
  port?: number
  password?: string
  public?: boolean
}

export interface LogsView {
  lines: string[]
}

export interface ModSearchResult {
  mod_id: string
  name: string
  owner: string
  version: string
  description: string
  downloads: number
}

export interface JobHandle {
  id: string
}

export type JobStatus =
  | { status: 'queued' }
  | { status: 'running' }
  | { status: 'succeeded' }
  | { status: 'failed'; message: string }

export interface JobSnapshot {
  id: string
  kind: JobKindDescr
  status: JobStatus
  log: string[]
}

export type JobKindDescr =
  | { kind: 'steamcmd_install' }
  | { kind: 'mod_add'; instance: string; mod_id: string }
  | { kind: 'mod_update'; instance: string }

export interface GlobalModInstanceEntry {
  instance: string
  version: string
  enabled: boolean
  running: boolean
}

export interface GlobalMod {
  mod_id: string
  global_version: string | null
  instances: GlobalModInstanceEntry[]
}

export interface ListView {
  ids: string[]
}

export interface ConfigFileEntry {
  filename: string
  size_bytes: number
}

export interface ConfigFileView {
  content: string
}

export type ListKind = 'admin' | 'banned' | 'permitted'

export interface HostResources {
  cpu_percent: number
  memory_total_bytes: number
  memory_used_bytes: number
  disk_total_bytes: number
  disk_available_bytes: number
}

export interface InstanceResources {
  running: boolean
  cpu_percent: number
  memory_bytes: number
}
