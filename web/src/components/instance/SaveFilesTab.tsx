import { ChevronRight, Download, File, Folder, Trash2, Upload } from 'lucide-react'
import { useRef } from 'react'
import { Link, Navigate } from 'react-router-dom'
import { toast } from 'sonner'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { buttonVariants } from '@/components/ui/button-variants'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ApiError } from '@/lib/api-client'
import { useDeleteSaveFile, useSaveFiles, useUploadSaveFiles } from '@/lib/queries'
import { cn, formatBytes, formatRelativeTime } from '@/lib/utils'

const urlFor = (basePath: string, segments: string[]) =>
  [basePath, ...segments.map(encodeURIComponent)].join('/')

export function SaveFilesTab({
  id,
  path,
  running,
  basePath,
}: {
  id: string
  path: string[]
  running: boolean
  basePath: string
}) {
  const folder = path.join('/')
  const files = useSaveFiles(id, folder)
  const upload = useUploadSaveFiles()
  const remove = useDeleteSaveFile()
  const fileInput = useRef<HTMLInputElement>(null)
  const { confirm, dialog } = useConfirmDialog()

  // A folder that no longer exists (or a malformed URL) falls back to its parent.
  if (path.length > 0 && files.error instanceof ApiError && [400, 404].includes(files.error.status)) {
    return <Navigate replace to={urlFor(basePath, path.slice(0, -1))} />
  }
  if (files.isError) return <QueryError error={files.error} />

  const handleUpload = async (selected: File[]) => {
    if (selected.length === 0) return
    const existing = new Set(files.data?.map((entry) => entry.name))
    const replaced = selected.filter((file) => existing.has(file.name)).map((file) => file.name)
    if (replaced.length > 0) {
      const confirmed = await confirm({
        title: 'Replace existing files?',
        description: `${replaced.join(', ')} already exist${replaced.length === 1 ? 's' : ''} in this folder and will be overwritten.`,
        confirmLabel: 'Replace',
      })
      if (!confirmed) return
    }
    upload.mutate(
      { id, path: folder, files: selected },
      {
        onSuccess: () => toast.success(`Uploaded ${selected.length} file${selected.length === 1 ? '' : 's'}`),
        onError: (error) => toast.error(error.message),
      },
    )
  }

  const handleDelete = async (entry: { path: string; name: string; kind: 'file' | 'dir' }) => {
    const isDir = entry.kind === 'dir'
    const confirmed = await confirm({
      title: `Delete ${isDir ? 'folder' : 'file'} '${entry.name}'?`,
      description: isDir
        ? 'The folder and everything inside it will be permanently deleted. This cannot be undone.'
        : 'The file will be permanently deleted. This cannot be undone.',
      confirmLabel: 'Delete',
    })
    if (!confirmed) return
    remove.mutate({ id, path: entry.path }, { onError: (error) => toast.error(error.message) })
  }

  return (
    <div className="flex flex-col gap-3">
      {dialog}
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <nav aria-label="Folder" className="flex flex-wrap items-center gap-1 text-sm">
          <Link className="font-medium hover:underline" to={urlFor(basePath, [])}>
            Saves
          </Link>
          {path.map((segment, index) => (
            <span key={index} className="flex items-center gap-1">
              <ChevronRight className="size-3.5 text-muted-foreground" />
              {index === path.length - 1 ? (
                <span aria-current="page" className="font-mono text-xs">{segment}</span>
              ) : (
                <Link className="font-mono text-xs hover:underline" to={urlFor(basePath, path.slice(0, index + 1))}>
                  {segment}
                </Link>
              )}
            </span>
          ))}
        </nav>
        <Button size="sm" variant="outline" disabled={running || upload.isPending} onClick={() => fileInput.current?.click()}>
          {upload.isPending ? <Spinner data-icon="inline-start" /> : <Upload data-icon="inline-start" />}
          Upload
        </Button>
        <input
          ref={fileInput}
          type="file"
          multiple
          hidden
          onChange={(event) => {
            void handleUpload(Array.from(event.target.files ?? []))
            event.target.value = ''
          }}
        />
      </div>

      {running ? (
        <p className="text-xs text-muted-foreground">Uploading and deleting are disabled while the server is running — stop it first.</p>
      ) : null}

      {files.isLoading ? <Skeleton className="h-28 w-full" /> : null}
      {files.data?.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {path.length === 0 ? 'No save files have been created yet.' : 'This folder is empty.'}
        </p>
      ) : null}
      {files.data && files.data.length > 0 ? (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-full">Name</TableHead>
              <TableHead className="hidden sm:table-cell">Size</TableHead>
              <TableHead className="hidden md:table-cell">Modified</TableHead>
              <TableHead className="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {files.data.map((entry) => {
              const isDir = entry.kind === 'dir'
              return (
                <TableRow key={entry.path}>
                  <TableCell className="font-mono text-xs">
                    {isDir ? (
                      <Link className="flex items-center gap-2 hover:underline" to={urlFor(basePath, entry.path.split('/'))}>
                        <Folder className="size-4 shrink-0 text-muted-foreground" />
                        {entry.name}
                      </Link>
                    ) : (
                      <span className="flex items-center gap-2">
                        <File className="size-4 shrink-0 text-muted-foreground" />
                        {entry.name}
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground sm:table-cell">
                    {isDir ? '—' : formatBytes(entry.size_bytes)}
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground md:table-cell">
                    {entry.modified_at ? formatRelativeTime(entry.modified_at) : 'Unknown'}
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-right">
                    {isDir ? null : (
                      <a
                        className={cn(buttonVariants({ variant: 'ghost', size: 'icon' }))}
                        href={`/api/instances/${encodeURIComponent(id)}/saves/${entry.path.split('/').map(encodeURIComponent).join('/')}`}
                        download
                        aria-label={`Download ${entry.name}`}
                      >
                        <Download className="size-4" />
                      </a>
                    )}
                    <Button
                      variant="ghost"
                      size="icon"
                      disabled={running || remove.isPending}
                      aria-label={`Delete ${entry.name}`}
                      onClick={() => void handleDelete(entry)}
                    >
                      <Trash2 className="size-4" />
                    </Button>
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      ) : null}
    </div>
  )
}
