import { Loader2 } from 'lucide-react'
import { type FormEvent, useEffect, useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { PasswordStrength } from '@/components/PasswordStrength'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { readInvitationToken } from '@/lib/invitation'
import { useAcceptInvitation, useInvitationPeek } from '@/lib/queries'

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex min-h-svh items-center justify-center px-4">
      <div className="flex w-full max-w-sm flex-col gap-4 rounded-xl border p-6">
        <div className="flex items-center gap-2">
          <img src="/logo.png" alt="" className="size-8 shrink-0" />
          <h1 className="text-lg font-semibold tracking-tight">Welcome to Odin</h1>
        </div>
        {children}
      </div>
    </div>
  )
}

function InvalidLink({ message }: { message: string }) {
  return (
    <Shell>
      <p role="alert" className="text-sm text-destructive">
        {message}
      </p>
      <p className="text-xs text-muted-foreground">
        Ask an administrator to send you a new invitation.
      </p>
      <Link to="/" className="self-center text-xs underline underline-offset-4">
        Go to sign in
      </Link>
    </Shell>
  )
}

/** Landing page of the emailed invitation link: the invited user chooses a password and is signed in. */
export function AcceptInvitation() {
  const location = useLocation()
  const navigate = useNavigate()
  // The token arrives in the URL fragment; keep it in state and drop it from the address bar.
  const [token] = useState(() => readInvitationToken(location.hash))
  useEffect(() => {
    if (location.hash) window.history.replaceState(null, '', location.pathname)
  }, [location.hash, location.pathname])

  const peek = useInvitationPeek(token)
  const accept = useAcceptInvitation()
  const [password, setPassword] = useState('')

  if (token === null) return <InvalidLink message="This invitation link is incomplete." />
  if (peek.isPending) {
    return (
      <Shell>
        <Loader2 className="size-6 animate-spin self-center text-muted-foreground" />
      </Shell>
    )
  }
  if (peek.isError) return <InvalidLink message="This invitation link is invalid or has expired." />

  const email = peek.data.email
  const submit = (event: FormEvent) => {
    event.preventDefault()
    accept.mutate({ token, password }, { onSuccess: () => navigate('/', { replace: true }) })
  }

  return (
    <div className="flex min-h-svh items-center justify-center px-4">
      <form onSubmit={submit} className="flex w-full max-w-sm flex-col gap-4 rounded-xl border p-6">
        <div className="flex items-center gap-2">
          <img src="/logo.png" alt="" className="size-8 shrink-0" />
          <h1 className="text-lg font-semibold tracking-tight">Welcome to Odin</h1>
        </div>
        <p className="text-xs text-muted-foreground">
          Choose a password for <span className="font-medium text-foreground">{email}</span>. You
          will be signed in right away.
        </p>
        <div className="flex flex-col gap-2">
          <Label htmlFor="invite-password">Password</Label>
          <Input
            id="invite-password"
            type="password"
            autoComplete="new-password"
            autoFocus
            required
            minLength={8}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <PasswordStrength password={password} identity={[email]} />
        </div>
        {accept.isError && (
          <p role="alert" className="text-sm text-destructive">
            {accept.error.message}
          </p>
        )}
        <Button type="submit" disabled={accept.isPending}>
          Set password and sign in
        </Button>
      </form>
    </div>
  )
}
