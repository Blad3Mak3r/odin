import { Loader2 } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { PageHeader } from '@/components/PageHeader'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { ACTIVITY_KIND_LABELS } from '@/lib/activity'
import {
  useCreateWebhook,
  useDeleteWebhook,
  useSetWebhookEnabled,
  useTestWebhook,
  useUpdateWebhook,
  useWebhooks,
} from '@/lib/queries'
import type { ActivityKind, WebhookView } from '@/lib/types'

const ACTIVITY_KINDS = Object.keys(ACTIVITY_KIND_LABELS) as ActivityKind['kind'][]

export function WebhooksPage() {
  const webhooks = useWebhooks()

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Webhooks"
        description="Send a message to Discord (or anything Discord-compatible) when something happens."
        action={<CreateWebhookDialog />}
      />

      {webhooks.isError && <QueryError error={webhooks.error} />}

      {webhooks.isLoading && (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-20 w-full" />
          <Skeleton className="h-20 w-full" />
        </div>
      )}

      {webhooks.data?.length === 0 && (
        <p className="text-sm text-muted-foreground">
          No webhooks yet — add one to get notified when a server crashes, a backup fails, or an
          update is available.
        </p>
      )}

      <div className="flex flex-col gap-2">
        {webhooks.data?.map((hook) => (
          <WebhookRow key={hook.id} webhook={hook} />
        ))}
      </div>
    </div>
  )
}

function WebhookRow({ webhook }: { webhook: WebhookView }) {
  const setEnabled = useSetWebhookEnabled()
  const deleteWebhook = useDeleteWebhook()
  const testWebhook = useTestWebhook()
  const { confirm, dialog } = useConfirmDialog()

  const handleDelete = async () => {
    const confirmed = await confirm({
      title: 'Remove webhook?',
      description: 'Stop sending activity events to this webhook.',
      confirmLabel: 'Remove',
    })
    if (!confirmed) return
    deleteWebhook.mutate(webhook.id, { onError: (e) => toast.error(e.message) })
  }

  const handleTest = () => {
    testWebhook.mutate(webhook.id, {
      onSuccess: () => toast.success('Test message sent'),
      onError: (e) => toast.error(e.message),
    })
  }

  return (
    <div className="flex flex-col gap-2 rounded-xl border p-3">
      {dialog}
      <div className="flex items-center justify-between gap-3">
        <span className="min-w-0 flex-1 truncate text-sm font-medium">Webhook #{webhook.id.slice(0, 8)}</span>
        <Switch
          checked={webhook.enabled}
          onCheckedChange={(enabled) =>
            setEnabled.mutate(
              { id: webhook.id, enabled },
              { onError: (e) => toast.error(e.message) },
            )
          }
        />
      </div>
      <div className="flex flex-wrap items-center gap-1">
        {webhook.event_kinds.length === 0 ? (
          <Badge variant="outline">All events</Badge>
        ) : (
          webhook.event_kinds.map((kind) => (
            <Badge key={kind} variant="outline">
              {ACTIVITY_KIND_LABELS[kind]}
            </Badge>
          ))
        )}
      </div>
      <div className="flex justify-end gap-2">
        <EditWebhookDialog webhook={webhook} />
        <Button size="sm" variant="outline" disabled={testWebhook.isPending} onClick={handleTest}>
          {testWebhook.isPending && <Loader2 className="size-4 animate-spin" />}
          Send test
        </Button>
        <Button
          size="sm"
          variant="destructive"
          disabled={deleteWebhook.isPending}
          onClick={handleDelete}
        >
          Remove
        </Button>
      </div>
    </div>
  )
}

function EditWebhookDialog({ webhook }: { webhook: WebhookView }) {
  const [open, setOpen] = useState(false)
  const [selectedKinds, setSelectedKinds] = useState<ActivityKind['kind'][]>(webhook.event_kinds)
  const updateWebhook = useUpdateWebhook()

  const toggleKind = (kind: ActivityKind['kind']) =>
    setSelectedKinds((prev) =>
      prev.includes(kind) ? prev.filter((current) => current !== kind) : [...prev, kind],
    )

  const handleOpenChange = (nextOpen: boolean) => {
    setOpen(nextOpen)
    if (nextOpen) setSelectedKinds(webhook.event_kinds)
  }

  const handleSave = () => {
    updateWebhook.mutate(
      { id: webhook.id, eventKinds: selectedKinds },
      {
        onSuccess: () => {
          setOpen(false)
          toast.success('Webhook events updated')
        },
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger render={<Button size="sm" variant="outline">Edit events</Button>} />
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Edit webhook events</DialogTitle>
          <DialogDescription>Choose which activity events this webhook receives.</DialogDescription>
        </DialogHeader>
        <EventKindFields selectedKinds={selectedKinds} toggleKind={toggleKind} idPrefix={`webhook-${webhook.id}`} />
        <DialogFooter>
          <Button disabled={updateWebhook.isPending} onClick={handleSave}>
            {updateWebhook.isPending && <Loader2 className="size-4 animate-spin" />}
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function CreateWebhookDialog() {
  const [open, setOpen] = useState(false)
  const [url, setUrl] = useState('')
  const [selectedKinds, setSelectedKinds] = useState<ActivityKind['kind'][]>([])
  const createWebhook = useCreateWebhook()

  const toggleKind = (kind: ActivityKind['kind']) =>
    setSelectedKinds((prev) =>
      prev.includes(kind) ? prev.filter((k) => k !== kind) : [...prev, kind],
    )

  const handleCreate = () => {
    createWebhook.mutate(
      { url, event_kinds: selectedKinds },
      {
        onSuccess: () => {
          setOpen(false)
          setUrl('')
          setSelectedKinds([])
          toast.success('Webhook added')
        },
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button>Add webhook</Button>} />
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add webhook</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          <Label htmlFor="webhook-url">URL</Label>
          <Input
            id="webhook-url"
            placeholder="https://discord.com/api/webhooks/…"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />
        </div>
        <EventKindFields selectedKinds={selectedKinds} toggleKind={toggleKind} idPrefix="create-webhook" />
        <DialogFooter>
          <Button disabled={!url.trim() || createWebhook.isPending} onClick={handleCreate}>
            {createWebhook.isPending && <Loader2 className="size-4 animate-spin" />}
            Add
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function EventKindFields({
  selectedKinds,
  toggleKind,
  idPrefix,
}: {
  selectedKinds: ActivityKind['kind'][]
  toggleKind: (kind: ActivityKind['kind']) => void
  idPrefix: string
}) {
  return (
    <FieldSet>
      <FieldLegend variant="label">Events</FieldLegend>
      <FieldDescription>Leave all unchecked to send every event.</FieldDescription>
      <FieldGroup className="grid gap-2 sm:grid-cols-2">
        {ACTIVITY_KINDS.map((kind) => {
          const id = `${idPrefix}-${kind}`
          return (
            <Field key={kind} orientation="horizontal">
              <Checkbox id={id} checked={selectedKinds.includes(kind)} onCheckedChange={() => toggleKind(kind)} />
              <FieldLabel htmlFor={id} className="font-normal">{ACTIVITY_KIND_LABELS[kind]}</FieldLabel>
            </Field>
          )
        })}
      </FieldGroup>
    </FieldSet>
  )
}
