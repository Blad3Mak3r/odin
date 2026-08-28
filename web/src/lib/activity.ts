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

export const ACTIVITY_ICONS: Record<ActivityKind['kind'], LucideIcon> = {
  instance_created: Plus,
  instance_deleted: Trash2,
  instance_started: Play,
  instance_stopped: Square,
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
