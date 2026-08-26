import { useId } from 'react'
import { Area, AreaChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
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
  const gradientId = `resource-chart-${useId().replace(/[^a-zA-Z0-9]/g, '')}`

  if (data.length < 2) {
    return (
      <div className="flex h-16 items-center justify-center text-xs text-muted-foreground">
        Collecting data…
      </div>
    )
  }

  return (
    <div className="h-16 w-full">
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={data} margin={{ top: 4, right: 4, bottom: 0, left: 4 }}>
          <defs>
            <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={color} stopOpacity={0.35} />
              <stop offset="100%" stopColor={color} stopOpacity={0} />
            </linearGradient>
          </defs>
          <XAxis dataKey="at" hide />
          <YAxis hide domain={[0, (max: number) => Math.max(max, 1)]} />
          <Tooltip
            formatter={(value) => formatValue(Number(value))}
            labelFormatter={(label) => new Date(String(label)).toLocaleTimeString()}
            contentStyle={{
              background: 'var(--popover)',
              color: 'var(--popover-foreground)',
              border: '1px solid var(--border)',
              borderRadius: 6,
              fontSize: 12,
            }}
            labelStyle={{ color: 'var(--muted-foreground)' }}
          />
          <Area
            type="monotone"
            dataKey={dataKey}
            stroke={color}
            strokeWidth={2}
            fill={`url(#${gradientId})`}
            dot={false}
            isAnimationActive={false}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  )
}
