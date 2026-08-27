import { ResourceChart } from '@/components/ResourceChart'
import { Skeleton } from '@/components/ui/skeleton'
import type { ResourceSample } from '@/lib/types'

export function ResourceMetric({
  label,
  value,
  history,
  dataKey,
  formatValue,
}: {
  label: string
  value: string
  history: ResourceSample[]
  dataKey: 'cpu_percent' | 'memory_bytes'
  formatValue: (value: number) => string
}) {
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center justify-between">
        <span className="text-muted-foreground">{label}</span>
        <span>{value}</span>
      </div>
      <ResourceChart data={history} dataKey={dataKey} formatValue={formatValue} />
    </div>
  )
}

export function ResourceMetricSkeleton() {
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center justify-between">
        <Skeleton className="h-4 w-16" />
        <Skeleton className="h-4 w-12" />
      </div>
      <Skeleton className="h-16 w-full" />
    </div>
  )
}
