// Mirrors the prefix-dispatch logic in `src/mods/source.rs`: a mod's source
// is encoded as a prefix on its `mod_id` rather than a separate field, so
// this is purely client-side string logic, not an API round trip.

export type ModSource = 'thunderstore' | 'nexus' | 'local'

export function getModSource(modId: string): ModSource {
  if (modId.startsWith('nexus:')) return 'nexus'
  if (modId.startsWith('local:')) return 'local'
  return 'thunderstore'
}

export const MOD_SOURCE_LABEL: Record<ModSource, string> = {
  thunderstore: 'Thunderstore',
  nexus: 'Nexus Mods',
  local: 'Local',
}
