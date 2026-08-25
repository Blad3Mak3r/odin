import { ModIcon } from '@/components/ModIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import type { ModSearchResult } from '@/lib/types'

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
        <ModIcon src={mod.icon} className="size-12" />
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
