import { Package } from 'lucide-react'
import { useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import type { ModSearchResult } from '@/lib/types'

function ModIcon({ src, alt }: { src: string | null; alt: string }) {
  const [failed, setFailed] = useState(false)

  if (!src || failed) {
    return (
      <div className="flex size-12 shrink-0 items-center justify-center rounded-md bg-muted">
        <Package className="size-6 text-muted-foreground" />
      </div>
    )
  }

  return (
    <img
      src={src}
      alt={alt}
      loading="lazy"
      onError={() => setFailed(true)}
      className="size-12 shrink-0 rounded-md object-cover"
    />
  )
}

export function ModSearchCard({
  mod,
  onSelect,
  selectLabel,
  disabled,
}: {
  mod: ModSearchResult
  onSelect: (mod: ModSearchResult) => void
  selectLabel: string
  disabled?: boolean
}) {
  return (
    <Card className="flex h-full flex-col">
      <CardHeader className="flex-row items-start gap-3 space-y-0">
        <ModIcon src={mod.icon} alt="" />
        <div className="min-w-0">
          <p className="truncate text-sm font-medium">{mod.name}</p>
          <p className="truncate text-xs text-muted-foreground">by {mod.owner}</p>
        </div>
      </CardHeader>
      <CardContent className="flex-1">
        <p className="line-clamp-2 text-xs text-muted-foreground">{mod.description}</p>
      </CardContent>
      <CardFooter className="flex flex-col items-stretch gap-2">
        <div className="flex flex-wrap gap-2">
          <Badge variant="outline">v{mod.version}</Badge>
          <Badge variant="outline">{mod.downloads.toLocaleString()} downloads</Badge>
        </div>
        <Button size="sm" disabled={disabled} onClick={() => onSelect(mod)}>
          {selectLabel}
        </Button>
      </CardFooter>
    </Card>
  )
}
