import { Loader2 } from 'lucide-react'
import { type FormEvent, type ReactNode, useState } from 'react'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuthenticate, useAuthStatus } from '@/lib/queries'

function AuthForm({ mode }: { mode: 'login' | 'setup' }) {
  const authenticate = useAuthenticate(mode)
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const setup = mode === 'setup'

  const submit = (event: FormEvent) => {
    event.preventDefault()
    authenticate.mutate({ username, password })
  }

  return (
    <div className="flex min-h-svh items-center justify-center px-4">
      <form onSubmit={submit} className="flex w-full max-w-sm flex-col gap-4 rounded-xl border p-6">
        <div className="flex items-center gap-2">
          <img src="/logo.png" alt="" className="size-8 shrink-0" />
          <h1 className="text-lg font-semibold tracking-tight">
            {setup ? 'Set up Odin' : 'Sign in to Odin'}
          </h1>
        </div>
        {setup && (
          <p className="text-xs text-muted-foreground">
            Create the administrator account. This is only possible once, before any account exists.
          </p>
        )}
        <div className="flex flex-col gap-2">
          <Label htmlFor="auth-username">Username</Label>
          <Input
            id="auth-username"
            autoComplete="username"
            autoFocus
            required
            value={username}
            onChange={(e) => setUsername(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="auth-password">Password</Label>
          <Input
            id="auth-password"
            type="password"
            autoComplete={setup ? 'new-password' : 'current-password'}
            required
            minLength={setup ? 8 : undefined}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          {setup && <p className="text-xs text-muted-foreground">At least 8 characters.</p>}
        </div>
        {authenticate.isError && (
          <p role="alert" className="text-sm text-destructive">
            {authenticate.error.message}
          </p>
        )}
        <Button type="submit" disabled={authenticate.isPending}>
          {setup ? 'Create administrator' : 'Sign in'}
        </Button>
      </form>
    </div>
  )
}

/** Shows the app only to a signed-in user; otherwise the setup or login form. */
export function AuthGate({ children }: { children: ReactNode }) {
  const status = useAuthStatus()
  if (status.isLoading) {
    return (
      <div className="flex min-h-svh items-center justify-center">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
      </div>
    )
  }
  if (status.isError) {
    return (
      <div className="mx-auto max-w-md p-6">
        <QueryError error={status.error} />
      </div>
    )
  }
  if (!status.data) return null
  if (status.data.setup_required) return <AuthForm mode="setup" />
  if (!status.data.user) return <AuthForm mode="login" />
  return <>{children}</>
}
