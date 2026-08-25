import { Package } from 'lucide-react'
import { useState } from 'react'
import { cn } from '@/lib/utils'

export function ModIcon({
  src,
  alt = '',
  className,
}: {
  src: string | null | undefined
  alt?: string
  className?: string
}) {
  const [failed, setFailed] = useState(false)

  if (!src || failed) {
    return (
      <div
        className={cn(
          'flex size-10 shrink-0 items-center justify-center rounded-md bg-muted',
          className,
        )}
      >
        <Package className="size-5 text-muted-foreground" />
      </div>
    )
  }

  return (
    <img
      src={src}
      alt={alt}
      loading="lazy"
      onError={() => setFailed(true)}
      className={cn('size-10 shrink-0 rounded-md object-cover', className)}
    />
  )
}
