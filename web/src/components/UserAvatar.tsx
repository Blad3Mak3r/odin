import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import type { AuthUser } from '@/lib/types'
import { cn } from '@/lib/utils'

function initials(user: Pick<AuthUser, 'email' | 'username'>) {
  return (user.username ?? user.email).slice(0, 2).toUpperCase()
}

/** A user's profile picture, falling back to their initials. */
export function UserAvatar({
  user,
  className,
}: {
  user: Pick<AuthUser, 'email' | 'username' | 'avatar_updated_at'>
  className?: string
}) {
  return (
    <Avatar className={cn('size-9', className)}>
      {user.avatar_updated_at && (
        <AvatarImage
          // The version in the URL makes a new picture bypass the browser cache.
          src={`/api/auth/avatar/${encodeURIComponent(user.email)}?v=${encodeURIComponent(user.avatar_updated_at)}`}
          alt=""
        />
      )}
      <AvatarFallback className="text-xs font-medium">{initials(user)}</AvatarFallback>
    </Avatar>
  )
}
