export interface VersionView {
  latest_version: string | null
  latest_release_url: string | null
  update_available: boolean
}

export interface CheckResult {
  label: string
  ok: boolean
  critical: boolean
  detail: string | null
}

export interface InstallStatusView {
  installed: boolean
  installed_build_id: number | null
  latest_build_id: number | null
  update_available: boolean
}

export interface InstalledMod {
  mod_id: string
  version: string
  installed_at: string
  enabled: boolean
  icon: string | null
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
  pid: number | null
  pid_started_at: number | null
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
  icon: string | null
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
  started_at: string
  log: string[]
}

export interface JobSummary {
  id: string
  kind: JobKindDescr
  status: JobStatus
  started_at: string
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
  icon: string | null
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

export interface ResourceSample {
  at: string
  cpu_percent: number
  memory_bytes: number
}

export interface PlayerInfo {
  name: string
  connected_at: string
}

export interface InstanceResourceEntry {
  name: string
  running: boolean
  cpu_percent: number
  memory_bytes: number
  players: PlayerInfo[]
}

export interface ResourcesTick {
  host: HostResources
  instances: InstanceResourceEntry[]
}

export type ActivityKind =
  | { kind: 'instance_created' }
  | { kind: 'instance_deleted' }
  | { kind: 'instance_started' }
  | { kind: 'instance_stopped' }
  | { kind: 'server_installed' }
  | { kind: 'mod_installed'; mod_id: string }
  | { kind: 'mod_removed'; mod_id: string }
  | { kind: 'mods_updated' }
  | { kind: 'player_joined'; name: string }
  | { kind: 'player_left'; name: string }

export interface ActivityEvent {
  id: string
  at: string
  instance: string | null
  kind: ActivityKind
}
