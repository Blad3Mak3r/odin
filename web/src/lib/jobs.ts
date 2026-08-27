import type { JobKindDescr, JobStatus } from './types'

export function describeJobKind(kind: JobKindDescr): string {
  switch (kind.kind) {
    case 'steamcmd_install':
      return 'Install/update server files'
    case 'mod_add':
      return `Install mod ${kind.mod_id} on ${kind.instance}`
    case 'mod_update':
      return `Update mods on ${kind.instance}`
    case 'backup_create':
      return `Back up ${kind.instance}`
    case 'backup_restore':
      return `Restore ${kind.instance} from backup ${kind.backup_id}`
    default:
      return (kind as { kind: string }).kind
  }
}

export function jobStatusVariant(status: JobStatus): 'destructive' | 'secondary' {
  return status.status === 'failed' ? 'destructive' : 'secondary'
}
