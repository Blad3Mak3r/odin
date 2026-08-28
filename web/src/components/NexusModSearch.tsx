import { Loader2 } from 'lucide-react'
import { useState } from 'react'
import { ModSearchCard } from '@/components/ModSearchCard'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { useNexusLookup, useNexusTrending } from '@/lib/queries'
import type { ModSearchResult } from '@/lib/types'

// Nexus Mods v3 has no keyword-search endpoint (unlike Thunderstore's flat
// index), so discovery here is: paste a mod URL/ID to resolve it directly,
// or browse a short "trending" list. Same `onSelect`/`selectLabel`/
// `selectDisabled` shape as `ModSearch` so call sites can swap between them.
export function NexusModSearch({
  onSelect,
  selectLabel = 'Install',
  selectDisabled,
}: {
  onSelect: (mod: ModSearchResult) => void
  selectLabel?: string
  selectDisabled?: (mod: ModSearchResult) => boolean
}) {
  const [query, setQuery] = useState('')
  const lookup = useNexusLookup()
  const trending = useNexusTrending()

  const handleLookup = () => {
    if (!query.trim()) return
    lookup.mutate(query.trim())
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-3">
        <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Look up a Nexus Mods mod
        </h2>
        <div className="flex gap-2">
          <Input
            placeholder="Paste a mod URL or id, e.g. nexusmods.com/valheim/mods/1234"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleLookup()}
          />
          <Button onClick={handleLookup} disabled={lookup.isPending || !query.trim()}>
            {lookup.isPending && <Loader2 className="size-4 animate-spin" />}
            Look up
          </Button>
        </div>
        {lookup.isError && (
          <p className="text-sm text-destructive">{lookup.error.message}</p>
        )}
        {lookup.data && (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            <ModSearchCard
              mod={lookup.data}
              selectLabel={selectLabel}
              disabled={selectDisabled?.(lookup.data)}
              onSelect={onSelect}
            />
          </div>
        )}
      </div>

      <div className="flex flex-col gap-3">
        <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Trending on Nexus
        </h2>

        {trending.isLoading && (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {Array.from({ length: 3 }, (_, i) => (
              // eslint-disable-next-line react/no-array-index-key
              <Skeleton key={i} className="h-40 rounded-xl" />
            ))}
          </div>
        )}
        {trending.isError && (
          <p className="text-sm text-destructive">{trending.error.message}</p>
        )}
        {trending.data?.length === 0 && (
          <p className="text-sm text-muted-foreground">No trending mods right now.</p>
        )}

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {trending.data?.map((mod) => (
            <ModSearchCard
              key={mod.mod_id}
              mod={mod}
              selectLabel={selectLabel}
              disabled={selectDisabled?.(mod)}
              onSelect={onSelect}
            />
          ))}
        </div>
      </div>
    </div>
  )
}
