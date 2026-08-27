import { useState } from 'react'
import { JobProgress } from '@/components/JobProgress'
import { PageHeader } from '@/components/PageHeader'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useJobSocket } from '@/hooks/useJobSocket'
import { describeJobKind, jobStatusVariant } from '@/lib/jobs'
import { useJobs } from '@/lib/queries'
import type { JobSummary } from '@/lib/types'
import { formatRelativeTime } from '@/lib/utils'

export function JobsPage() {
  const jobs = useJobs()
  const [selectedJob, setSelectedJob] = useState<JobSummary | null>(null)

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Jobs"
        description="Server installs, updates, and mod operations, with their live logs."
      />

      {jobs.isLoading && (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
        </div>
      )}

      {jobs.isError && <QueryError error={jobs.error} />}

      {jobs.data && jobs.data.length === 0 && (
        <p className="text-sm text-muted-foreground">No jobs yet.</p>
      )}

      {jobs.data && jobs.data.length > 0 && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-full">Job</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="hidden sm:table-cell">Started</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {jobs.data.map((job) => (
              <TableRow
                key={job.id}
                className="cursor-pointer"
                onClick={() => setSelectedJob(job)}
              >
                <TableCell>{describeJobKind(job.kind)}</TableCell>
                <TableCell>
                  <Badge variant={jobStatusVariant(job.status)}>{job.status.status}</Badge>
                </TableCell>
                <TableCell className="hidden text-muted-foreground sm:table-cell">
                  {formatRelativeTime(job.started_at)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      <JobDetailDialog job={selectedJob} onClose={() => setSelectedJob(null)} />
    </div>
  )
}

function JobDetailDialog({ job, onClose }: { job: JobSummary | null; onClose: () => void }) {
  const socket = useJobSocket(job?.id ?? null)

  return (
    <Dialog open={job !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{job && describeJobKind(job.kind)}</DialogTitle>
        </DialogHeader>
        {job && (
          <JobProgress
            log={socket.log}
            status={socket.status}
            connected={socket.connected}
            logHeightClassName="max-h-96"
          />
        )}
      </DialogContent>
    </Dialog>
  )
}
