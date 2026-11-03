import {
  type ChangeEvent,
  type KeyboardEvent,
  type PointerEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react'
import { toast } from 'sonner'
import { UserAvatar } from '@/components/UserAvatar'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { useDeleteAvatar, useUploadAvatar } from '@/lib/queries'
import type { AuthUser } from '@/lib/types'

/** Matches the server's upload limit. */
const MAX_FILE_BYTES = 5 * 1024 * 1024
/** Edge of the crop area on screen, and of the square sent to the server. */
const VIEW = 256
const OUTPUT = 512
const MAX_ZOOM = 4
const KEY_STEP = 8

interface Offset {
  x: number
  y: number
}

const clamp = (value: number, limit: number) => Math.max(-limit, Math.min(limit, value))

function CropDialog({
  file,
  pending,
  onCancel,
  onConfirm,
}: {
  file: File | null
  pending: boolean
  onCancel: () => void
  onConfirm: (image: Blob) => void
}) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const drag = useRef<{ x: number; y: number } | null>(null)
  const [image, setImage] = useState<HTMLImageElement | null>(null)
  const [zoom, setZoom] = useState(1)
  const [offset, setOffset] = useState<Offset>({ x: 0, y: 0 })

  useEffect(() => {
    if (!file) return
    const url = URL.createObjectURL(file)
    const img = new Image()
    img.onload = () => {
      setZoom(1)
      setOffset({ x: 0, y: 0 })
      setImage(img)
    }
    img.onerror = () => {
      toast.error('That file could not be read as an image')
      onCancel()
    }
    img.src = url
    return () => {
      URL.revokeObjectURL(url)
      setImage(null)
    }
  }, [file, onCancel])

  // The image covers the square at zoom 1; zooming in only adds slack to pan.
  const scale = image ? (VIEW / Math.min(image.width, image.height)) * zoom : 1
  const slack = image
    ? { x: (image.width * scale - VIEW) / 2, y: (image.height * scale - VIEW) / 2 }
    : { x: 0, y: 0 }
  const at: Offset = { x: clamp(offset.x, slack.x), y: clamp(offset.y, slack.y) }

  const paint = (target: HTMLCanvasElement, edge: number) => {
    const ctx = target.getContext('2d')
    if (!ctx || !image) return
    const ratio = edge / VIEW
    const width = image.width * scale * ratio
    const height = image.height * scale * ratio
    ctx.clearRect(0, 0, edge, edge)
    ctx.imageSmoothingQuality = 'high'
    ctx.drawImage(
      image,
      edge / 2 + at.x * ratio - width / 2,
      edge / 2 + at.y * ratio - height / 2,
      width,
      height,
    )
  }

  useEffect(() => {
    if (canvas.current) paint(canvas.current, VIEW)
  })

  const move = (dx: number, dy: number) =>
    setOffset({ x: clamp(at.x + dx, slack.x), y: clamp(at.y + dy, slack.y) })

  const onPointerDown = (event: PointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId)
    drag.current = { x: event.clientX, y: event.clientY }
  }
  const onPointerMove = (event: PointerEvent<HTMLCanvasElement>) => {
    if (!drag.current) return
    move(event.clientX - drag.current.x, event.clientY - drag.current.y)
    drag.current = { x: event.clientX, y: event.clientY }
  }
  const onKeyDown = (event: KeyboardEvent<HTMLCanvasElement>) => {
    const arrows: Record<string, [number, number]> = {
      ArrowLeft: [-KEY_STEP, 0],
      ArrowRight: [KEY_STEP, 0],
      ArrowUp: [0, -KEY_STEP],
      ArrowDown: [0, KEY_STEP],
    }
    const step = arrows[event.key]
    if (!step) return
    event.preventDefault()
    move(...step)
  }

  const confirm = () => {
    const out = document.createElement('canvas')
    out.width = OUTPUT
    out.height = OUTPUT
    paint(out, OUTPUT)
    out.toBlob((blob) => (blob ? onConfirm(blob) : toast.error('Could not prepare the image')), 'image/png')
  }

  return (
    <Dialog open={file !== null} onOpenChange={(open) => !open && !pending && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Adjust your photo</DialogTitle>
          <DialogDescription>
            Drag the image (or use the arrow keys) to frame it, and zoom to fit.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col items-center gap-4">
          <div className="relative overflow-hidden rounded-md bg-muted" style={{ width: VIEW, height: VIEW }}>
            <canvas
              ref={canvas}
              width={VIEW}
              height={VIEW}
              tabIndex={0}
              aria-label="Photo crop area"
              className="block cursor-grab touch-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring active:cursor-grabbing"
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={() => (drag.current = null)}
              onPointerCancel={() => (drag.current = null)}
              onKeyDown={onKeyDown}
            />
            <div className="pointer-events-none absolute inset-0 rounded-full shadow-[0_0_0_999px_rgba(0,0,0,0.5)]" />
          </div>
          <div className="flex w-full max-w-xs items-center gap-3">
            <Label htmlFor="avatar-zoom" className="text-xs text-muted-foreground">
              Zoom
            </Label>
            <input
              id="avatar-zoom"
              type="range"
              min={1}
              max={MAX_ZOOM}
              step={0.01}
              value={zoom}
              onChange={(e) => setZoom(Number(e.target.value))}
              className="w-full accent-primary"
            />
          </div>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" disabled={pending} onClick={onCancel}>
            Cancel
          </Button>
          <Button type="button" disabled={!image || pending} onClick={confirm}>
            Save photo
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function ProfilePicture({ user }: { user: AuthUser }) {
  const upload = useUploadAvatar()
  const remove = useDeleteAvatar()
  const input = useRef<HTMLInputElement>(null)
  const [picked, setPicked] = useState<File | null>(null)
  const cancel = useCallback(() => setPicked(null), [])

  const choose = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (!file) return
    if (!file.type.startsWith('image/')) {
      toast.error('Choose an image file')
    } else if (file.size > MAX_FILE_BYTES) {
      toast.error('The image is too large (5 MB maximum)')
    } else {
      setPicked(file)
    }
  }

  const save = (image: Blob) =>
    upload.mutate(image, {
      onSuccess: () => {
        setPicked(null)
        toast.success('Profile picture updated')
      },
      onError: (e) => toast.error(e.message),
    })

  return (
    <div className="flex max-w-md items-center gap-4 rounded-xl border p-4">
      <UserAvatar user={user} className="size-20" />
      <div className="flex min-w-0 flex-col gap-2">
        <h2 className="text-sm font-medium">Profile picture</h2>
        <p className="text-xs text-muted-foreground">
          JPEG, PNG, GIF or WebP. It is resized and stored as a small WebP.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button type="button" size="sm" onClick={() => input.current?.click()}>
            {user.avatar_updated_at ? 'Change photo' : 'Upload photo'}
          </Button>
          {user.avatar_updated_at && (
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={remove.isPending}
              onClick={() =>
                remove.mutate(undefined, {
                  onSuccess: () => toast.success('Profile picture removed'),
                  onError: (e) => toast.error(e.message),
                })
              }
            >
              Remove
            </Button>
          )}
        </div>
        <input
          ref={input}
          type="file"
          accept="image/*"
          className="hidden"
          onChange={choose}
        />
      </div>
      <CropDialog
        file={picked}
        pending={upload.isPending}
        onCancel={cancel}
        onConfirm={save}
      />
    </div>
  )
}
