import { Download, FileArchive } from 'lucide-react'
import { QueryError } from '@/components/QueryError'
import { buttonVariants } from '@/components/ui/button-variants'
import { Skeleton } from '@/components/ui/skeleton'
import { useSaveFiles } from '@/lib/queries'
import { cn, formatBytes, formatRelativeTime } from '@/lib/utils'

export function SaveFilesTab({ id }: { id: string }) {
  const files = useSaveFiles(id)

  if (files.isError) return <QueryError error={files.error} />
  if (files.isLoading) return <Skeleton className="h-28 w-full" />
  if (!files.data?.length) {
    return <p className="text-sm text-muted-foreground">No save files have been created yet.</p>
  }

  return (
    <div className="overflow-x-auto rounded-xl border">
      <table className="w-full text-left text-sm">
        <thead className="border-b bg-muted/40 text-xs text-muted-foreground">
          <tr>
            <th className="px-3 py-2 font-medium">File</th>
            <th className="px-3 py-2 font-medium">Size</th>
            <th className="px-3 py-2 font-medium">Modified</th>
            <th className="px-3 py-2"><span className="sr-only">Download</span></th>
          </tr>
        </thead>
        <tbody>
          {files.data.map((file) => {
            const encodedPath = file.path.split('/').map(encodeURIComponent).join('/')
            const href = `/api/instances/${encodeURIComponent(id)}/saves/${encodedPath}`
            return (
              <tr key={file.path} className="border-b last:border-0">
                <td className="px-3 py-2 font-mono text-xs">
                  <span className="flex items-center gap-2">
                    <FileArchive className="size-4 shrink-0 text-muted-foreground" />
                    {file.path}
                  </span>
                </td>
                <td className="whitespace-nowrap px-3 py-2">{formatBytes(file.size_bytes)}</td>
                <td className="whitespace-nowrap px-3 py-2 text-muted-foreground">
                  {file.modified_at ? formatRelativeTime(file.modified_at) : 'Unknown'}
                </td>
                <td className="px-3 py-2 text-right">
                  <a
                    className={cn(buttonVariants({ variant: 'ghost', size: 'icon' }))}
                    href={href}
                    download
                    aria-label={`Download ${file.path}`}
                  >
                    <Download className="size-4" />
                  </a>
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
