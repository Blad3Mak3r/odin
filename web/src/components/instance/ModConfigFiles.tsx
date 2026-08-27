import { StreamLanguage } from '@codemirror/language'
import { properties } from '@codemirror/legacy-modes/mode/properties'
import { yaml } from '@codemirror/legacy-modes/mode/yaml'
import { EditorView } from '@codemirror/view'
import { githubDark, githubLight } from '@uiw/codemirror-theme-github'
import CodeMirror from '@uiw/react-codemirror'
import { Loader2 } from 'lucide-react'
import { useTheme } from 'next-themes'
import { useCallback, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Skeleton } from '@/components/ui/skeleton'
import { useConfigFileContent, useConfigFiles, useSetConfigFileContent } from '@/lib/queries'

// Module-scope so these extension instances are referentially stable across
// renders — CodeMirror reconfigures on every `extensions` array identity
// change, so a new array is fine, but its contents shouldn't be.
const CFG_LANGUAGE = StreamLanguage.define(properties)
const YAML_LANGUAGE = StreamLanguage.define(yaml)

function languageFor(filename: string) {
  return filename.endsWith('.yml') || filename.endsWith('.yaml') ? YAML_LANGUAGE : CFG_LANGUAGE
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  return `${(bytes / 1024).toFixed(1)} KB`
}

export function ModConfigFiles({ name }: { name: string }) {
  const files = useConfigFiles(name)
  const [editingFilename, setEditingFilename] = useState<string | null>(null)

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
        Configuration files
      </h2>

      {files.isLoading && (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-14 w-full" />
          <Skeleton className="h-14 w-full" />
        </div>
      )}

      {files.data?.length === 0 && (
        <p className="text-sm text-muted-foreground">
          No BepInEx config files found yet. Config files appear here once a mod generates one on
          first server start.
        </p>
      )}

      <div className="flex flex-col gap-2">
        {files.data?.map((file) => (
          <div
            key={file.filename}
            className="flex items-center justify-between rounded-xl border p-3"
          >
            <div>
              <p className="text-sm font-medium">{file.filename}</p>
              <p className="text-xs text-muted-foreground">{formatSize(file.size_bytes)}</p>
            </div>
            <Button size="sm" variant="outline" onClick={() => setEditingFilename(file.filename)}>
              Edit
            </Button>
          </div>
        ))}
      </div>

      {editingFilename && (
        <ConfigFileEditDialog
          key={editingFilename}
          name={name}
          filename={editingFilename}
          onClose={() => setEditingFilename(null)}
        />
      )}
    </div>
  )
}

function ConfigFileEditDialog({
  name,
  filename,
  onClose,
}: {
  name: string
  filename: string
  onClose: () => void
}) {
  const { resolvedTheme } = useTheme()
  const content = useConfigFileContent(name, filename)
  const setContent = useSetConfigFileContent(name)
  const [draft, setDraft] = useState<string | null>(null)

  const original = content.data?.content ?? null
  const value = draft ?? original ?? ''
  const dirty = draft !== null && draft !== original

  const extensions = useMemo(
    () => [languageFor(filename), EditorView.lineWrapping],
    [filename],
  )
  const handleChange = useCallback((next: string) => setDraft(next), [])

  const handleSave = () => {
    if (draft === null) return
    setContent.mutate(
      { filename, content: draft },
      {
        onSuccess: () => {
          toast.success(`Saved ${filename}`)
          onClose()
        },
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{filename}</DialogTitle>
        </DialogHeader>

        {content.isLoading && <p className="text-sm text-muted-foreground">Loading…</p>}

        {content.isError && (
          <p className="text-sm text-destructive">
            This config file no longer exists — it may have been removed or regenerated under a
            different name.
          </p>
        )}

        {original !== null && (
          <div className="min-w-0 overflow-hidden rounded-xl border">
            <CodeMirror
              value={value}
              height="60vh"
              theme={resolvedTheme === 'dark' ? githubDark : githubLight}
              extensions={extensions}
              onChange={handleChange}
            />
          </div>
        )}

        <DialogFooter className="items-center">
          {dirty && (
            <p className="mr-auto text-xs text-muted-foreground">
              Unsaved changes are discarded on close.
            </p>
          )}
          <Button
            disabled={!dirty || setContent.isPending}
            onClick={handleSave}
          >
            {setContent.isPending && <Loader2 className="size-4 animate-spin" />}
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
