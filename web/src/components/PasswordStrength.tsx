import { assessPassword } from '@/lib/passwordStrength'
import { cn } from '@/lib/utils'

const BAR_COLOR = ['bg-destructive', 'bg-amber-500', 'bg-lime-500', 'bg-green-600']

/** Strength bar and hint under a new-password field. `identity` is the email and username to keep out of it. */
export function PasswordStrength({
  password,
  identity,
}: {
  password: string
  identity?: string[]
}) {
  const strength = assessPassword(password, identity)
  if (!strength) return null
  return (
    <div role="status" aria-live="polite" className="flex flex-col gap-1">
      <div aria-hidden className="flex gap-1">
        {BAR_COLOR.map((color, index) => (
          <span
            key={color}
            className={cn('h-1 flex-1 rounded-full bg-muted', index <= strength.score && color)}
          />
        ))}
      </div>
      <p className="text-xs text-muted-foreground">
        <span className="font-medium text-foreground">{strength.label}</span> · {strength.hint}
      </p>
    </div>
  )
}
