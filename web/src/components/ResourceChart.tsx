import { useMemo } from 'react'
import { areaY, defineChart } from '@tanstack/charts'
import { Chart } from '@tanstack/charts/react'
import { scaleLinear } from '@tanstack/charts/scales/linear'
import { d3Curve } from '@tanstack/charts/d3/shape'
import { tooltip } from '@tanstack/charts/tooltip'
import { curveMonotoneX } from 'd3-shape'
import type { ResourceSample } from '@/lib/types'

interface ResourceChartProps {
  data: ResourceSample[]
  dataKey: 'cpu_percent' | 'memory_bytes'
  formatValue: (value: number) => string
  color?: string
}

/// A small sparkline-style area chart for one resource metric over time —
/// used for both host and per-instance CPU/memory history. A single series
/// per chart, so no legend: the surrounding card title already names it.
export function ResourceChart({
  data,
  dataKey,
  formatValue,
  color = 'var(--color-primary)',
}: ResourceChartProps) {
  const definition = useMemo(() => {
    const rows = data.map((sample) => ({ at: Date.parse(sample.at), value: sample[dataKey] }))
    const max = Math.max(1, ...rows.map((row) => row.value))
    return defineChart({
      marks: [
        areaY(rows, {
          x: 'at',
          y: 'value',
          fill: color,
          fillOpacity: 0.25,
          stroke: color,
          strokeWidth: 2,
          curve: d3Curve(curveMonotoneX),
        }),
      ],
      scales: {
        x: { scale: scaleLinear, axis: false },
        y: { scale: scaleLinear().domain([0, max]), axis: false },
      },
      margin: { top: 4, right: 4, bottom: 0, left: 4 },
      tooltip: {
        use: tooltip,
        items: [
          { channel: 'y', label: 'Value', text: (point) => formatValue(point.yValue) },
          { channel: 'x', label: 'Time', text: (point) => new Date(point.xValue).toLocaleTimeString() },
        ],
      },
      focus: 'nearest-x',
      maxFocusDistance: Number.POSITIVE_INFINITY,
    })
  }, [data, dataKey, formatValue, color])

  if (data.length < 2) {
    return (
      <div className="flex h-16 items-center justify-center text-xs text-muted-foreground">
        Collecting data…
      </div>
    )
  }

  return (
    <Chart
      definition={definition}
      height={64}
      ariaLabel={dataKey === 'cpu_percent' ? 'CPU usage history' : 'Memory usage history'}
      className="resource-chart w-full"
    />
  )
}
