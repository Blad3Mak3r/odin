import { useEffect, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
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

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-sm font-medium">Search Thunderstore</h2>
      <Input
        placeholder="Search mods by name or author…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />

      {results.isLoading && <p className="text-sm text-muted-foreground">Searching…</p>}

      <div className="flex flex-col gap-2">
        {results.data?.slice(0, 20).map((mod) => (
          <div
            key={mod.mod_id}
            className="flex flex-col gap-2 rounded-md border p-3 sm:flex-row sm:items-center sm:justify-between"
          >
            <div>
              <p className="text-sm font-medium">
                {mod.name} <span className="text-muted-foreground">by {mod.owner}</span>
              </p>
              <p className="line-clamp-1 text-xs text-muted-foreground">{mod.description}</p>
              <div className="mt-1 flex gap-2">
                <Badge variant="outline">v{mod.version}</Badge>
                <Badge variant="outline">{mod.downloads.toLocaleString()} downloads</Badge>
              </div>
            </div>
            <Button size="sm" disabled={selectDisabled?.(mod)} onClick={() => onSelect(mod)}>
              {selectLabel}
            </Button>
          </div>
        ))}
      </div>
    </div>
  )
}
