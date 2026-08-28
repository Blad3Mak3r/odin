import {
  Archive,
  ArchiveRestore,
  ArchiveX,
  CircleCheck,
  type LucideIcon,
  Package,
  PackageMinus,
  PackagePlus,
  Play,
  Plus,
  RefreshCcw,
  Square,
  Trash2,
  UserMinus,
  UserPlus,
} from 'lucide-react'
import type { ActivityKind } from './types'

export function describeActivity(kind: ActivityKind): string {
  switch (kind.kind) {
    case 'instance_created':
      return 'Instance created'
    case 'instance_deleted':
      return 'Instance deleted'
    case 'instance_started':
      return 'Instance started'
    case 'instance_stopped':
      return 'Instance stopped'
    case 'instance_auto_restarted':
      return 'Instance restarted automatically after crashing'
    case 'server_installed':
      return 'Server files installed/updated'
    case 'mod_installed':
      return `Mod installed: ${kind.mod_id}`
    case 'mod_removed':
      return `Mod removed: ${kind.mod_id}`
    case 'mods_updated':
      return 'Mods updated'
    case 'backup_created':
      return `Backup created: ${kind.backup_id}`
    case 'backup_restored':
      return `Restored from backup: ${kind.backup_id}`
    case 'backup_pruned':
      return `Old backup pruned: ${kind.backup_id}`
    case 'player_joined':
      return `${kind.name} joined`
    case 'player_left':
      return `${kind.name} left`
  }
}

// Short, payload-independent labels for picking which activity kinds a
// webhook should forward — unlike `describeActivity`, these don't need a
// specific event's fields (a mod id, a backup id, ...) to read sensibly.
export const ACTIVITY_KIND_LABELS: Record<ActivityKind['kind'], string> = {
  instance_created: 'Instance created',
  instance_deleted: 'Instance deleted',
  instance_started: 'Instance started',
  instance_stopped: 'Instance stopped',
  instance_auto_restarted: 'Auto-restarted after a crash',
  server_installed: 'Server files installed/updated',
  mod_installed: 'Mod installed',
  mod_removed: 'Mod removed',
  mods_updated: 'Mods updated',
  backup_created: 'Backup created',
  backup_restored: 'Backup restored',
  backup_pruned: 'Old backup pruned',
  player_joined: 'Player joined',
  player_left: 'Player left',
}

export const ACTIVITY_ICONS: Record<ActivityKind['kind'], LucideIcon> = {
  instance_created: Plus,
  instance_deleted: Trash2,
  instance_started: Play,
  instance_stopped: Square,
  instance_auto_restarted: RefreshCcw,
  server_installed: CircleCheck,
  mod_installed: PackagePlus,
  mod_removed: PackageMinus,
  mods_updated: Package,
  backup_created: Archive,
  backup_restored: ArchiveRestore,
  backup_pruned: ArchiveX,
  player_joined: UserPlus,
  player_left: UserMinus,
}
