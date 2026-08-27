import { useEffect, useState } from 'react'
import { ModSearchCard } from '@/components/ModSearchCard'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { useModSearch } from '@/lib/queries'
import type { ModSearchResult } from '@/lib/types'

const SEARCH_DEBOUNCE_MS = 300

function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value)

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delayMs)
    return () => clearTimeout(timer)
  }, [value, delayMs])

  return debounced
}

export function ModSearch({
  onSelect,
  selectLabel = 'Install',
  selectDisabled,
}: {
  onSelect: (mod: ModSearchResult) => void
  selectLabel?: string
  selectDisabled?: (mod: ModSearchResult) => boolean
}) {
  const [query, setQuery] = useState('')
  const debouncedQuery = useDebouncedValue(query, SEARCH_DEBOUNCE_MS)
  const results = useModSearch(debouncedQuery)

  const hasQuery = debouncedQuery.trim().length > 0
  const noResults = hasQuery && !results.isLoading && results.data?.length === 0

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
        Search Thunderstore
      </h2>
      <Input
        placeholder="Search mods by name or author…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />

      {results.isLoading && (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }, (_, i) => (
            // Placeholder skeletons have no stable id and never reorder.
            // eslint-disable-next-line react/no-array-index-key
            <Skeleton key={i} className="h-40 rounded-xl" />
          ))}
        </div>
      )}

      {noResults && (
        <p className="text-sm text-muted-foreground">
          No mods found for &lsquo;{debouncedQuery}&rsquo;.
        </p>
      )}

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {results.data?.slice(0, 20).map((mod) => (
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
  )
}
