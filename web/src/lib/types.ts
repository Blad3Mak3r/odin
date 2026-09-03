export interface VersionView {
  latest_version: string | null
  latest_release_url: string | null
  update_available: boolean
  outdated_instances: string[]
}

export type GameId = 'valheim' | 'rust'

export interface GameCapabilities {
  backups: boolean
  players: boolean
  mods: boolean
  access_lists: boolean
  readiness: boolean
}

export interface GameView {
  id: GameId
  name: string
  steam_app_id: string
  capabilities: GameCapabilities
}

export interface ManagedInstanceView {
  id: string
  game: GameId
  name: string
  created_at: string
  running: boolean
  capabilities: GameCapabilities
  config: Record<string, unknown>
}

export interface RustConfigUpdateRequest {
  hostname?: string
  level?: string
  seed?: number
  world_size?: number
  max_players?: number
  auto_restart?: boolean
}

export interface ChangelogSection {
  title: string
  changes: string[]
}

export interface ChangelogRelease {
  version: string
  date: string | null
  sections: ChangelogSection[]
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

export interface BepInExStatus {
  installed: boolean
  installed_version: string | null
  latest_version: string | null
  update_available: boolean
}

export interface InstalledMod {
  mod_id: string
  version: string
  installed_at: string
  enabled: boolean
  pinned: boolean
  available_versions: string[]
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
  odin_version: string | null
}

export interface ConfigView {
  world_name: string
  port: number
  password: string | null
  public: boolean
  auto_restart: boolean
}

export interface ConfigUpdateRequest {
  world?: string
  port?: number
  password?: string
  public?: boolean
  auto_restart?: boolean
}

export interface LogsView {
  lines: string[]
}

export interface LastExitInfo {
  code: number | null
  at: string
  recent_lines: string[]
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
  | { kind: 'steamcmd_install'; game: GameId }
  | { kind: 'mod_add'; instance: string; mod_id: string }
  | { kind: 'mod_update'; instance: string }
  | { kind: 'mod_upload'; instance: string; name: string }
  | { kind: 'backup_create'; instance: string }
  | { kind: 'backup_restore'; instance: string; backup_id: string }
  | { kind: 'bepinex_update'; instance: string; from_version: string | null; to_version: string }

export interface GlobalModInstanceEntry {
  instance: string
  version: string
  enabled: boolean
  pinned: boolean
  running: boolean
}

export interface GlobalMod {
  mod_id: string
  stored_versions: string[]
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

export interface BackupEntry {
  id: string
  created_at: string
  size_bytes: number
  storage: 'local' | BackupStorageProvider
}

export interface BackupScheduleView {
  interval_hours: number
  retain_count: number
  enabled: boolean
  last_run_at: string | null
}

export type BackupStorageProvider = 'aws_s3' | 'cloudflare_r2'

export interface BackupStorageView {
  configured: boolean
  enabled: boolean
  provider: BackupStorageProvider | null
  endpoint: string
  region: string
  bucket: string
  prefix: string
  access_key_id: string
  secret_access_key_configured: boolean
}

export interface BackupStorageRequest {
  enabled: boolean
  provider: BackupStorageProvider
  endpoint: string | null
  region: string | null
  bucket: string
  prefix: string
  access_key_id: string
  secret_access_key: string | null
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
  ready: boolean
  cpu_percent: number
  memory_bytes: number
}

export type InstanceTransition = 'starting' | 'stopping' | 'restarting' | 'cloning' | 'updating_bepinex'
export type InstanceTransitions = Record<string, InstanceTransition>

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
  game: GameId
  name: string
  running: boolean
  ready: boolean
  cpu_percent: number
  memory_bytes: number
  players: PlayerInfo[]
  last_saved_at: string | null
}

export interface ResourcesTick {
  host: HostResources
  instances: InstanceResourceEntry[]
}

export type ActivityKind =
  | { kind: 'instance_created' }
  | { kind: 'instance_cloned'; source: string }
  | { kind: 'instance_deleted' }
  | { kind: 'instance_started' }
  | { kind: 'instance_stopped' }
  | { kind: 'instance_auto_restarted' }
  | { kind: 'server_installed' }
  | { kind: 'server_update_available'; installed_build_id: number; latest_build_id: number }
  | { kind: 'mod_installed'; mod_id: string }
  | { kind: 'mod_removed'; mod_id: string }
  | { kind: 'mods_updated' }
  | { kind: 'bepinex_updated'; from_version: string | null; to_version: string }
  | { kind: 'backup_created'; backup_id: string }
  | { kind: 'backup_restored'; backup_id: string }
  | { kind: 'backup_pruned'; backup_id: string }
  | { kind: 'player_joined'; name: string }
  | { kind: 'player_left'; name: string }

export interface ActivityEvent {
  id: string
  at: string
  game: GameId
  instance: string | null
  instance_id?: string
  kind: ActivityKind
}

export interface BulkResult {
  name: string
  ok: boolean
  error: string | null
}

export interface BulkBepInExResult {
  name: string
  job_id: string | null
  error: string | null
}

export interface WebhookView {
  id: string
  enabled: boolean
  event_kinds: ActivityKind['kind'][]
  created_at: string
}

export interface SettingsView {
  nexus_api_key_configured: boolean
}
