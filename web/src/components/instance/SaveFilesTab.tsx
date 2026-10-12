import { ChevronRight, Download, File, Folder, Trash2, Upload } from 'lucide-react'
import { useRef, useState } from 'react'
import { toast } from 'sonner'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { buttonVariants } from '@/components/ui/button-variants'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useDeleteSaveFile, useSaveFiles, useUploadSaveFiles } from '@/lib/queries'
import { cn, formatBytes, formatRelativeTime } from '@/lib/utils'

export function SaveFilesTab({ id, running }: { id: string; running: boolean }) {
  // The folder being browsed is local UI state: the tab's URL stays `/saves`.
  const [path, setPath] = useState<string[]>([])
  const folder = path.join('/')
  const files = useSaveFiles(id, folder)
  const upload = useUploadSaveFiles()
  const remove = useDeleteSaveFile()
  const fileInput = useRef<HTMLInputElement>(null)
  const { confirm, dialog } = useConfirmDialog()

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
      <div className="flex flex-col border">
        <div className="flex flex-col gap-2 border-b bg-muted/50 px-3 py-2 sm:flex-row sm:items-center sm:justify-between">
          <nav aria-label="Folder" className="flex flex-wrap items-center gap-1 text-sm">
            <button type="button" className="font-medium hover:underline" onClick={() => setPath([])}>
              Saves
            </button>
            {path.map((segment, index) => (
              <span key={index} className="flex items-center gap-1">
                <ChevronRight className="size-3.5 text-muted-foreground" />
                {index === path.length - 1 ? (
                  <span aria-current="page" className="font-mono text-xs">{segment}</span>
                ) : (
                  <button type="button" className="font-mono text-xs hover:underline" onClick={() => setPath(path.slice(0, index + 1))}>
                    {segment}
                  </button>
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

        {/* Fixed-height body: navigating folders must not change the page height, or the page scrolls. */}
        <div
          className={cn('h-80 overflow-y-auto', files.isPlaceholderData && 'opacity-60')}
          aria-busy={files.isFetching}
        >
          {files.isError ? <QueryError error={files.error} /> : null}
          {files.isLoading ? <Skeleton className="h-full w-full" /> : null}
          {files.data?.length === 0 ? (
            <p className="p-3 text-sm text-muted-foreground">
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
                          <button
                            type="button"
                            className="flex items-center gap-2 hover:underline"
                            onClick={() => setPath(entry.path.split('/'))}
                          >
                            <Folder className="size-4 shrink-0 text-muted-foreground" />
                            {entry.name}
                          </button>
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
      </div>

      {running ? (
        <p className="text-xs text-muted-foreground">Uploading and deleting are disabled while the server is running — stop it first.</p>
      ) : null}
    </div>
  )
}
