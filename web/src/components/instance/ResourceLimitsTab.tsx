import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { QueryError } from '@/components/QueryError'
import { useResourceLimits, useSetResourceLimits } from '@/lib/queries'
import type { ResourceLimits } from '@/lib/types'

const GIB = 1024 ** 3

export function ResourceLimitsTab({ id }: { id: string }) {
  const limits = useResourceLimits(id)
  const save = useSetResourceLimits(id)

  if (limits.isError) return <QueryError error={limits.error} />
  if (limits.isLoading || !limits.data) {
    return <div className="flex max-w-md flex-col gap-4"><Skeleton className="h-14 w-full" /><Skeleton className="h-14 w-full" /><Skeleton className="h-12 w-32" /></div>
  }

  return <ResourceLimitsForm key={JSON.stringify(limits.data)} limits={limits.data} pending={save.isPending} onSave={(next) => save.mutate(next, {
    onSuccess: () => toast.success('Resource limits saved — they apply on the next start'),
    onError: (error) => toast.error(error.message),
  })} />
}

function ResourceLimitsForm({ limits, pending, onSave }: { limits: ResourceLimits; pending: boolean; onSave: (limits: ResourceLimits) => void }) {
  const [cpuPercent, setCpuPercent] = useState(limits.cpu_percent === null ? '' : String(limits.cpu_percent))
  const [memoryGiB, setMemoryGiB] = useState(limits.memory_max_bytes === null ? '' : String(limits.memory_max_bytes / GIB))
  const cpu = cpuPercent.trim() === '' ? null : Number(cpuPercent)
  const memory = memoryGiB.trim() === '' ? null : Number(memoryGiB)
  const cpuInvalid = cpu !== null && (!Number.isFinite(cpu) || cpu <= 0)
  const memoryInvalid = memory !== null && (!Number.isFinite(memory) || memory <= 0 || !Number.isSafeInteger(Math.round(memory * GIB)))
  const invalid = cpuInvalid || memoryInvalid

  return (
    <form className="flex max-w-md flex-col gap-4" onSubmit={(event) => {
      event.preventDefault()
      if (invalid) return
      onSave({
        cpu_percent: cpu,
        memory_max_bytes: memory === null ? null : Math.round(memory * GIB),
      })
    }}>
      <p className="text-sm text-muted-foreground">Limits apply the next time this server starts, including after an automatic restart. Leave a field blank to keep that resource unlimited.</p>
      <div className="flex flex-col gap-2">
        <Label htmlFor="cpu-limit">CPU limit (%)</Label>
        <Input id="cpu-limit" type="number" min="0.001" step="any" value={cpuPercent} onChange={(event) => setCpuPercent(event.target.value)} placeholder="Unlimited" />
        <p className="text-xs text-muted-foreground">100% is one full CPU core; values above 100% allow more than one core.</p>
        {cpuInvalid && <p className="text-xs text-destructive">Enter a CPU percentage greater than zero.</p>}
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="memory-limit">Memory limit (GiB)</Label>
        <Input id="memory-limit" type="number" min="0.001" step="any" value={memoryGiB} onChange={(event) => setMemoryGiB(event.target.value)} placeholder="Unlimited" />
        <p className="text-xs text-muted-foreground">At 90% Odin applies memory pressure; at 100% the kernel may terminate game processes to enforce the hard limit.</p>
        {memoryInvalid && <p className="text-xs text-destructive">Enter a memory limit greater than zero.</p>}
      </div>
      <Button className="w-fit" type="submit" disabled={pending || invalid}>Save limits</Button>
    </form>
  )
}
