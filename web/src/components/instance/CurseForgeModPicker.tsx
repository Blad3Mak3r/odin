import { ArrowDown, ArrowUp, ExternalLink, Search, X } from 'lucide-react'
import { useState } from 'react'
import { ModIcon } from '@/components/ModIcon'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import {
  useCurseForgeFeatured,
  useCurseForgeLookup,
  useCurseForgeMods,
  useCurseForgeSearch,
  useCurseForgeStatus,
} from '@/lib/queries'
import type { CurseForgeMod } from '@/lib/types'

// ARK accepts at most this many mods (see `ArkAsaOptions::validate`).
const MAX_MODS = 64

const compact = new Intl.NumberFormat(undefined, { notation: 'compact' })

type FeaturedTab = 'popular' | 'recently_updated' | 'featured'
const FEATURED_TABS: { id: FeaturedTab; label: string }[] = [
  { id: 'popular', label: 'Popular' },
  { id: 'recently_updated', label: 'Recently updated' },
  { id: 'featured', label: 'Featured' },
]

function parseIds(value: string): number[] {
  return value
    .split(',')
    .map((part) => Number(part.trim()))
    .filter((id) => Number.isSafeInteger(id) && id > 0)
}

// Numeric ids and mod page URLs are resolved directly; anything else is a search.
function isReference(query: string) {
  return /^\d+$/.test(query) || /curseforge\.com/i.test(query)
}

// `value` is the comma-separated id list the surrounding form already stores
// and validates, so the picker is a drop-in for the plain text input. Without
// a CurseForge API key on the server it falls back to exactly that input.
export function CurseForgeModPicker({
  id,
  value,
  onChange,
  disabled,
}: {
  id: string
  value: string
  onChange: (value: string) => void
  disabled?: boolean
}) {
  const status = useCurseForgeStatus()

  if (status.isLoading) return <Skeleton className="h-8" />
  if (!status.data?.configured) {
    return (
      <div className="flex flex-col gap-1">
        <Input
          id={id}
          placeholder="CurseForge IDs, comma-separated"
          value={value}
          disabled={disabled}
          onChange={(event) => onChange(event.target.value)}
        />
        <p className="text-xs text-muted-foreground">
          Set <code>[curseforge] api_key</code> in Odin&apos;s config.toml to search CurseForge from here.
        </p>
      </div>
    )
  }
  return <Picker id={id} ids={parseIds(value)} onChange={(ids) => onChange(ids.join(', '))} disabled={disabled} />
}

function Picker({
  id,
  ids,
  onChange,
  disabled,
}: {
  id: string
  ids: number[]
  onChange: (ids: number[]) => void
  disabled?: boolean
}) {
  const [query, setQuery] = useState('')
  const [submitted, setSubmitted] = useState('')
  const [tab, setTab] = useState<FeaturedTab>('popular')
  const selected = useCurseForgeMods(ids)
  const lookup = useCurseForgeLookup()
  const search = useCurseForgeSearch(submitted)
  const featured = useCurseForgeFeatured()

  const full = ids.length >= MAX_MODS
  const add = (mod: CurseForgeMod) => {
    if (!ids.includes(mod.id) && !full) onChange([...ids, mod.id])
  }
  const move = (index: number, by: -1 | 1) => {
    const next = [...ids]
    ;[next[index], next[index + by]] = [next[index + by], next[index]]
    onChange(next)
  }
  const submit = () => {
    const trimmed = query.trim()
    lookup.reset()
    if (isReference(trimmed)) {
      setSubmitted('')
      lookup.mutate(trimmed)
    } else {
      setSubmitted(trimmed)
    }
  }
  const clear = () => {
    setQuery('')
    setSubmitted('')
    lookup.reset()
  }

  const byId = new Map(selected.data?.map((mod) => [mod.id, mod]))
  const searchMods = search.data?.pages.flatMap((page) => page.mods) ?? []
  const featuredMods = featured.data?.[tab] ?? []

  let results: React.ReactNode
  if (lookup.isPending || (submitted && search.isLoading) || (!submitted && !lookup.data && !lookup.isError && featured.isLoading)) {
    results = <Skeleton className="h-16" />
  } else if (lookup.isError) {
    results = <p className="text-sm text-destructive">{lookup.error.message}</p>
  } else if (lookup.data) {
    results = <ModRow mod={lookup.data} added={ids.includes(lookup.data.id)} disabled={disabled || full} onAdd={add} />
  } else if (submitted) {
    results = search.isError ? (
      <p className="text-sm text-destructive">{search.error.message}</p>
    ) : (
      <>
        {searchMods.length === 0 && <p className="text-sm text-muted-foreground">No mods found for “{submitted}”.</p>}
        {searchMods.map((mod) => (
          <ModRow key={mod.id} mod={mod} added={ids.includes(mod.id)} disabled={disabled || full} onAdd={add} />
        ))}
        {search.hasNextPage && (
          <Button type="button" variant="outline" size="sm" className="w-fit" disabled={search.isFetchingNextPage} onClick={() => search.fetchNextPage()}>
            {search.isFetchingNextPage ? 'Loading…' : 'Load more'}
          </Button>
        )}
      </>
    )
  } else if (featured.isError) {
    results = <p className="text-sm text-destructive">{featured.error.message}</p>
  } else {
    results = (
      <>
        <div className="flex flex-wrap gap-1">
          {FEATURED_TABS.map((option) => (
            <Button key={option.id} type="button" size="xs" variant={tab === option.id ? 'secondary' : 'ghost'} onClick={() => setTab(option.id)}>
              {option.label}
            </Button>
          ))}
        </div>
        {featuredMods.map((mod) => (
          <ModRow key={mod.id} mod={mod} added={ids.includes(mod.id)} disabled={disabled || full} onAdd={add} />
        ))}
      </>
    )
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-2">
        <p className="text-xs text-muted-foreground">
          {ids.length} / {MAX_MODS} mods, loaded in this order
        </p>
        {ids.length === 0 && <p className="text-sm text-muted-foreground">No mods selected.</p>}
        {ids.map((modId, index) => {
          const mod = byId.get(modId)
          return (
            <div key={modId} className="flex items-center gap-2 rounded-md border p-2">
              <ModIcon src={mod?.icon} className="size-8" />
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">{mod?.name ?? `Mod ${modId}`}</p>
                <p className="truncate text-xs text-muted-foreground">
                  {mod ? `by ${mod.author || 'unknown'} · ` : selected.isFetching ? '' : 'Not found on CurseForge · '}ID {modId}
                </p>
              </div>
              <Button type="button" variant="ghost" size="icon-xs" aria-label={`Move ${mod?.name ?? modId} up`} disabled={disabled || index === 0} onClick={() => move(index, -1)}>
                <ArrowUp />
              </Button>
              <Button type="button" variant="ghost" size="icon-xs" aria-label={`Move ${mod?.name ?? modId} down`} disabled={disabled || index === ids.length - 1} onClick={() => move(index, 1)}>
                <ArrowDown />
              </Button>
              <Button type="button" variant="ghost" size="icon-xs" aria-label={`Remove ${mod?.name ?? modId}`} disabled={disabled} onClick={() => onChange(ids.filter((other) => other !== modId))}>
                <X />
              </Button>
            </div>
          )
        })}
      </div>

      <div className="flex gap-2">
        <Input
          id={id}
          placeholder="Search CurseForge, or paste a mod ID or URL"
          value={query}
          disabled={disabled}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return
            event.preventDefault()
            submit()
          }}
        />
        <Button type="button" variant="outline" disabled={disabled || !query.trim()} onClick={submit}>
          <Search /> Search
        </Button>
        {(submitted || lookup.data || lookup.isError) && (
          <Button type="button" variant="ghost" onClick={clear}>
            Clear
          </Button>
        )}
      </div>
      {full && <p className="text-xs text-muted-foreground">The mod limit is reached; remove one to add another.</p>}
      <div className="flex max-h-96 flex-col gap-2 overflow-y-auto">{results}</div>
    </div>
  )
}

function ModRow({ mod, added, disabled, onAdd }: { mod: CurseForgeMod; added: boolean; disabled?: boolean; onAdd: (mod: CurseForgeMod) => void }) {
  return (
    <div className="flex items-start gap-3 rounded-md border p-2">
      <ModIcon src={mod.icon} className="size-10" />
      <div className="min-w-0 flex-1">
        <p className="flex items-center gap-1 text-sm font-medium">
          <span className="truncate">{mod.name}</span>
          {mod.url && (
            <a href={mod.url} target="_blank" rel="noreferrer" aria-label={`Open ${mod.name} on CurseForge`} className="shrink-0 text-muted-foreground hover:text-foreground">
              <ExternalLink className="size-3" />
            </a>
          )}
        </p>
        <p className="truncate text-xs text-muted-foreground">
          by {mod.author || 'unknown'} · {compact.format(mod.downloads)} downloads · ID {mod.id}
        </p>
        <p className="line-clamp-2 text-xs text-muted-foreground">{mod.summary}</p>
      </div>
      <Button type="button" size="sm" variant={added ? 'secondary' : 'default'} disabled={disabled || added} onClick={() => onAdd(mod)}>
        {added ? 'Added' : 'Add'}
      </Button>
    </div>
  )
}
