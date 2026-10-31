import { Loader2 } from 'lucide-react'
import { type FormEvent, type ReactNode, useState } from 'react'
import { PasswordStrength } from '@/components/PasswordStrength'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuthenticate, useAuthStatus, useChangePassword, useLogout } from '@/lib/queries'

type Mode = 'login' | 'setup' | 'recover'

const TOKEN_HELP: Record<Exclude<Mode, 'login'>, string> = {
  setup:
    'Odin printed a one-time setup token when it started. Find it in the service log (for example `journalctl -u odin`) or the container logs.',
  recover:
    'Odin printed a one-time recovery token when it started with the recovery marker present. Find it in the service log.',
}

function AuthForm({ mode: initialMode, recoveryActive }: { mode: Mode; recoveryActive: boolean }) {
  const [mode, setMode] = useState<Mode>(initialMode)
  const authenticate = useAuthenticate(mode)
  const [token, setToken] = useState('')
  const [email, setEmail] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(true)
  const login = mode === 'login'

  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (mode === 'login') authenticate.mutate({ login: email, password, remember })
    else if (mode === 'setup')
      authenticate.mutate({ token, email, username: username.trim() || undefined, password })
    else authenticate.mutate({ token, email, password })
  }

  const title = { login: 'Sign in to Odin', setup: 'Set up Odin', recover: 'Recover access' }[mode]

  return (
    <div className="flex min-h-svh items-center justify-center px-4">
      <form onSubmit={submit} className="flex w-full max-w-sm flex-col gap-4 rounded-xl border p-6">
        <div className="flex items-center gap-2">
          <img src="/logo.png" alt="" className="size-8 shrink-0" />
          <h1 className="text-lg font-semibold tracking-tight">{title}</h1>
        </div>
        {!login && (
          <>
            <p className="text-xs text-muted-foreground">
              {mode === 'setup'
                ? 'Create the administrator account. This is only possible once, before any account exists.'
                : 'Choose a new password for an existing account. All of its sessions are signed out.'}
            </p>
            <div className="flex flex-col gap-2">
              <Label htmlFor="auth-token">{mode === 'setup' ? 'Setup token' : 'Recovery token'}</Label>
              <Input
                id="auth-token"
                autoComplete="off"
                spellCheck={false}
                required
                value={token}
                onChange={(e) => setToken(e.target.value)}
              />
              <p className="text-xs text-muted-foreground">{TOKEN_HELP[mode]}</p>
            </div>
          </>
        )}
        <div className="flex flex-col gap-2">
          <Label htmlFor="auth-email">{login ? 'Email or username' : 'Email'}</Label>
          <Input
            id="auth-email"
            type={login ? 'text' : 'email'}
            autoComplete={login ? 'username' : 'email'}
            autoFocus={login}
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
        </div>
        {mode === 'setup' && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="auth-username">Username (optional)</Label>
            <Input
              id="auth-username"
              autoComplete="username"
              pattern="[A-Za-z0-9._\-]{3,32}"
              title="3-32 letters, digits, '.', '_' or '-'"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
            <p className="text-xs text-muted-foreground">An alternative to your email when signing in.</p>
          </div>
        )}
        <div className="flex flex-col gap-2">
          <Label htmlFor="auth-password">{mode === 'recover' ? 'New password' : 'Password'}</Label>
          <Input
            id="auth-password"
            type="password"
            autoComplete={login ? 'current-password' : 'new-password'}
            required
            minLength={login ? undefined : 8}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          {!login && <PasswordStrength password={password} identity={[email, username]} />}
        </div>
        {login && (
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={remember} onCheckedChange={setRemember} />
            Keep me signed in on this device
          </label>
        )}
        {authenticate.isError && (
          <p role="alert" className="text-sm text-destructive">
            {authenticate.error.message}
          </p>
        )}
        <Button type="submit" disabled={authenticate.isPending}>
          {{ login: 'Sign in', setup: 'Create administrator', recover: 'Reset password' }[mode]}
        </Button>
        {mode !== 'setup' && (recoveryActive || mode === 'recover') && (
          <Button
            type="button"
            variant="link"
            className="h-auto self-center p-0 text-xs"
            onClick={() => {
              authenticate.reset()
              setMode(login ? 'recover' : 'login')
            }}
          >
            {login ? 'Recover access with a recovery token' : 'Back to sign in'}
          </Button>
        )}
      </form>
    </div>
  )
}

/** Shown instead of the app while the password was chosen by someone else. */
function ForcedPasswordChange() {
  const change = useChangePassword()
  const logout = useLogout()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')

  const submit = (event: FormEvent) => {
    event.preventDefault()
    change.mutate({ current_password: current, new_password: next })
  }

  return (
    <div className="flex min-h-svh items-center justify-center px-4">
      <form onSubmit={submit} className="flex w-full max-w-sm flex-col gap-4 rounded-xl border p-6">
        <div className="flex items-center gap-2">
          <img src="/logo.png" alt="" className="size-8 shrink-0" />
          <h1 className="text-lg font-semibold tracking-tight">Choose a new password</h1>
        </div>
        <p className="text-xs text-muted-foreground">
          An administrator set your password. Replace it with one only you know to continue.
        </p>
        <div className="flex flex-col gap-2">
          <Label htmlFor="forced-current">Current password</Label>
          <Input
            id="forced-current"
            type="password"
            autoComplete="current-password"
            autoFocus
            required
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="forced-new">New password</Label>
          <Input
            id="forced-new"
            type="password"
            autoComplete="new-password"
            required
            minLength={8}
            value={next}
            onChange={(e) => setNext(e.target.value)}
          />
          <PasswordStrength password={next} />
          <p className="text-xs text-muted-foreground">Different from the current one.</p>
        </div>
        {change.isError && (
          <p role="alert" className="text-sm text-destructive">
            {change.error.message}
          </p>
        )}
        <Button type="submit" disabled={change.isPending}>
          Change password
        </Button>
        <Button
          type="button"
          variant="link"
          className="h-auto self-center p-0 text-xs"
          onClick={() => logout.mutate()}
        >
          Sign out
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
  if (status.data.setup_required) return <AuthForm mode="setup" recoveryActive={false} />
  if (!status.data.user) return <AuthForm mode="login" recoveryActive={status.data.recovery_active} />
  if (status.data.user.must_change_password) return <ForcedPasswordChange />
  return <>{children}</>
}
