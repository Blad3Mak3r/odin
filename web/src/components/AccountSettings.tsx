import { type FormEvent, useState } from 'react'
import { toast } from 'sonner'
import { useConfirmDialog } from '@/components/ConfirmDialog'
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
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  useAuditLog,
  useAuthStatus,
  useChangePassword,
  useCreateUser,
  useDeleteUser,
  useRevokeOtherSessions,
  useRevokeSession,
  useUpdateProfile,
  useSessions,
  useUpdateUser,
  useUsers,
} from '@/lib/queries'
import type { AuditEvent, AuthUser, UserRole } from '@/lib/types'

function ProfileForm({ user }: { user: AuthUser }) {
  const update = useUpdateProfile()
  const [email, setEmail] = useState(user.email)
  const [username, setUsername] = useState(user.username ?? '')
  const [password, setPassword] = useState('')
  const emailChanged = email.trim().toLowerCase() !== user.email
  const usernameChanged = username.trim() !== (user.username ?? '')

  const submit = (event: FormEvent) => {
    event.preventDefault()
    update.mutate(
      {
        ...(emailChanged ? { email, current_password: password } : {}),
        ...(usernameChanged ? { username: username.trim() } : {}),
      },
      {
        onSuccess: (saved) => {
          setEmail(saved.email)
          setUsername(saved.username ?? '')
          setPassword('')
          toast.success('Profile saved')
        },
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex max-w-md flex-col gap-4 rounded-xl border p-4">
      <h2 className="text-sm font-medium">Profile</h2>
      <div className="flex flex-col gap-2">
        <Label htmlFor="profile-email">Email</Label>
        <Input
          id="profile-email"
          type="email"
          autoComplete="email"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="profile-username">Username (optional)</Label>
        <Input
          id="profile-username"
          autoComplete="username"
          pattern="[A-Za-z0-9._\-]{3,32}"
          title="3-32 letters, digits, '.', '_' or '-'"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
        />
      </div>
      {emailChanged && (
        <div className="flex flex-col gap-2">
          <Label htmlFor="profile-password">Current password</Label>
          <Input
            id="profile-password"
            type="password"
            autoComplete="current-password"
            required
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <p className="text-xs text-muted-foreground">Needed to change your email address.</p>
        </div>
      )}
      <Button
        type="submit"
        className="w-fit"
        disabled={update.isPending || (!emailChanged && !usernameChanged)}
      >
        Save profile
      </Button>
    </form>
  )
}

function ChangePasswordForm() {
  const change = useChangePassword()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')

  const submit = (event: FormEvent) => {
    event.preventDefault()
    change.mutate(
      { current_password: current, new_password: next },
      {
        onSuccess: () => {
          setCurrent('')
          setNext('')
          toast.success('Password changed')
        },
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <form onSubmit={submit} className="flex max-w-md flex-col gap-4 rounded-xl border p-4">
      <h2 className="text-sm font-medium">Change password</h2>
      <div className="flex flex-col gap-2">
        <Label htmlFor="current-password">Current password</Label>
        <Input
          id="current-password"
          type="password"
          autoComplete="current-password"
          required
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
        />
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="new-password">New password</Label>
        <Input
          id="new-password"
          type="password"
          autoComplete="new-password"
          required
          minLength={8}
          value={next}
          onChange={(e) => setNext(e.target.value)}
        />
      </div>
      <Button type="submit" className="w-fit" disabled={change.isPending}>
        Change password
      </Button>
    </form>
  )
}

function describeAgent(agent: string | null) {
  if (!agent) return 'Unknown device'
  const browser =
    /Edg\//.test(agent) ? 'Edge'
    : /OPR\/|Opera/.test(agent) ? 'Opera'
    : /Firefox\//.test(agent) ? 'Firefox'
    : /Chrome\//.test(agent) ? 'Chrome'
    : /Safari\//.test(agent) ? 'Safari'
    : /curl|wget|python|node/i.test(agent) ? 'Script'
    : 'Browser'
  const system =
    /Windows/.test(agent) ? 'Windows'
    : /Android/.test(agent) ? 'Android'
    : /iPhone|iPad|iOS/.test(agent) ? 'iOS'
    : /Mac OS X|Macintosh/.test(agent) ? 'macOS'
    : /Linux|X11/.test(agent) ? 'Linux'
    : null
  return system ? `${browser} on ${system}` : browser
}

function SessionsPanel() {
  const sessions = useSessions()
  const revoke = useRevokeSession()
  const revokeOthers = useRevokeOtherSessions()
  const onError = (e: Error) => toast.error(e.message)
  const others = sessions.data?.filter((session) => !session.current).length ?? 0

  return (
    <section className="max-w-2xl border">
      <div className="flex items-start justify-between gap-3 border-b px-4 py-3">
        <div>
          <h2 className="text-sm font-medium">Active sessions</h2>
          <p className="mt-1 text-xs text-muted-foreground">
            Devices signed in to your account. Sessions end after a period without use.
          </p>
        </div>
        <Button
          className="shrink-0"
          variant="outline"
          size="sm"
          disabled={others === 0 || revokeOthers.isPending}
          onClick={() =>
            revokeOthers.mutate(undefined, {
              onSuccess: () => toast.success('Signed out of all other sessions'),
              onError,
            })
          }
        >
          Sign out others
        </Button>
      </div>
      <div className="divide-y">
        {sessions.data?.map((session) => (
          <div key={session.id} className="flex flex-wrap items-center gap-2 px-4 py-3">
            <span className="mr-auto min-w-0">
              <span className="flex items-center gap-2 font-medium">
                {describeAgent(session.user_agent)}
                {session.current && <Badge>this session</Badge>}
                {!session.remember && <Badge variant="outline">not remembered</Badge>}
              </span>
              <span className="block truncate text-xs text-muted-foreground">
                {session.ip ?? 'unknown address'} · last active{' '}
                {new Date(session.last_used_at).toLocaleString()}
              </span>
            </span>
            <Button
              variant="outline"
              size="sm"
              disabled={revoke.isPending}
              onClick={() => revoke.mutate(session.id, { onError })}
            >
              {session.current ? 'Sign out' : 'Revoke'}
            </Button>
          </div>
        ))}
      </div>
    </section>
  )
}

function ResetPasswordDialog({
  user,
  currentUserEmail,
  onClose,
}: {
  user: AuthUser | null
  currentUserEmail: string
  onClose: () => void
}) {
  const update = useUpdateUser()
  const [password, setPassword] = useState('')
  // Unset until the admin touches it: forced for someone else's account, not for their own.
  const [chosen, setChosen] = useState<boolean | null>(null)
  const requireChange = chosen ?? user?.email !== currentUserEmail
  const name = user?.username ?? user?.email

  const close = () => {
    setPassword('')
    setChosen(null)
    update.reset()
    onClose()
  }

  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (!user) return
    update.mutate(
      { email: user.email, password, require_change: requireChange },
      {
        onSuccess: () => {
          toast.success(`Password reset for ${name}`)
          close()
        },
      },
    )
  }

  return (
    <Dialog open={user !== null} onOpenChange={(open) => !open && close()}>
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>Reset password for {name}</DialogTitle>
            <DialogDescription>
              They are signed out everywhere and must use this password next time.
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-2">
            <Label htmlFor="reset-password">New password</Label>
            <Input
              id="reset-password"
              type="password"
              autoComplete="new-password"
              required
              minLength={8}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={requireChange} onCheckedChange={setChosen} />
            Require a new password at next sign-in
          </label>
          {update.isError && (
            <p role="alert" className="text-sm text-destructive">
              {update.error.message}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" disabled={update.isPending}>
              Reset password
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

const AUDIT_LABELS: Record<string, string> = {
  setup_completed: 'First administrator created',
  recovery_used: 'Access recovered',
  bootstrap_token_failed: 'Wrong setup or recovery token',
  login_succeeded: 'Signed in',
  login_failed: 'Failed sign-in',
  login_throttled: 'Sign-in blocked (too many attempts)',
  password_changed: 'Password changed',
  password_reset: 'Password reset',
  user_created: 'User created',
  user_deleted: 'User deleted',
  role_changed: 'Role changed',
  session_revoked: 'Session revoked',
  other_sessions_revoked: 'Other sessions revoked',
  profile_updated: 'Profile updated',
  account_disabled: 'Account disabled',
  account_enabled: 'Account enabled',
}

const AUDIT_ALERTS = new Set(['bootstrap_token_failed', 'login_failed', 'login_throttled'])

function auditSummary(event: AuditEvent) {
  const who = [event.actor, event.target && `→ ${event.target}`].filter(Boolean).join(' ')
  return [who, event.detail].filter(Boolean).join(' · ')
}

function SecurityLog() {
  const log = useAuditLog()
  return (
    <section className="max-w-2xl border">
      <div className="flex items-start justify-between gap-3 border-b px-4 py-3">
        <div>
          <h2 className="text-sm font-medium">Security log</h2>
          <p className="mt-1 text-xs text-muted-foreground">
            Sign-ins, resets and account changes, newest first. Kept for 90 days.
          </p>
        </div>
        <Button className="shrink-0" variant="outline" size="sm" onClick={() => log.refetch()}>
          Refresh
        </Button>
      </div>
      <div className="max-h-96 divide-y overflow-y-auto">
        {log.data?.length === 0 && (
          <p className="px-4 py-3 text-sm text-muted-foreground">No events yet.</p>
        )}
        {log.data?.map((event) => (
          <div key={event.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2 text-sm">
            <Badge variant={AUDIT_ALERTS.has(event.kind) ? 'destructive' : 'secondary'}>
              {AUDIT_LABELS[event.kind] ?? event.kind}
            </Badge>
            <span className="min-w-0 flex-1 truncate text-muted-foreground">{auditSummary(event)}</span>
            <span className="text-xs text-muted-foreground">
              {event.ip ? `${event.ip} · ` : ''}
              {new Date(event.at).toLocaleString()}
            </span>
          </div>
        ))}
      </div>
    </section>
  )
}

function UsersPanel({ currentUserEmail }: { currentUserEmail: string }) {
  const users = useUsers()
  const create = useCreateUser()
  const update = useUpdateUser()
  const remove = useDeleteUser()
  const { confirm, dialog } = useConfirmDialog()
  const [email, setEmail] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [role, setRole] = useState<UserRole>('viewer')
  const [requireChange, setRequireChange] = useState(true)
  const [resetting, setResetting] = useState<AuthUser | null>(null)

  const onError = (e: Error) => toast.error(e.message)

  const submit = (event: FormEvent) => {
    event.preventDefault()
    create.mutate(
      { email, username: username.trim() || undefined, password, role, require_change: requireChange },
      {
        onSuccess: () => {
          setEmail('')
          setUsername('')
          setPassword('')
          toast.success(`User ${email} created`)
        },
        onError,
      },
    )
  }

  return (
    <section className="max-w-2xl border">
      {dialog}
      <ResetPasswordDialog
        user={resetting}
        currentUserEmail={currentUserEmail}
        onClose={() => setResetting(null)}
      />
      <div className="border-b px-4 py-3">
        <h2 className="text-sm font-medium">Users</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          Administrators can change anything. Viewers can see the dashboard but not modify it.
        </p>
      </div>
      <div className="divide-y">
        {users.data?.map((user) => (
          <div key={user.email} className="flex flex-wrap items-center gap-2 px-4 py-3">
            <span className="mr-auto min-w-0">
              <span className="block truncate font-medium">{user.username ?? user.email}</span>
              {user.username && (
                <span className="block truncate text-xs text-muted-foreground">{user.email}</span>
              )}
              <span className="block text-xs text-muted-foreground">
                {user.last_login_at
                  ? `Last signed in ${new Date(user.last_login_at).toLocaleString()}`
                  : 'Never signed in'}
              </span>
            </span>
            {user.disabled && <Badge variant="destructive">disabled</Badge>}
            {user.must_change_password && <Badge variant="outline">must change password</Badge>}
            <Badge variant={user.role === 'admin' ? 'default' : 'secondary'}>{user.role}</Badge>
            <Button
              variant="outline"
              size="sm"
              disabled={update.isPending}
              onClick={() =>
                update.mutate(
                  { email: user.email, role: user.role === 'admin' ? 'viewer' : 'admin' },
                  { onError },
                )
              }
            >
              Make {user.role === 'admin' ? 'viewer' : 'admin'}
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={user.email === currentUserEmail || update.isPending}
              onClick={() =>
                update.mutate({ email: user.email, disabled: !user.disabled }, { onError })
              }
            >
              {user.disabled ? 'Enable' : 'Disable'}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setResetting(user)}
            >
              Reset password
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={user.email === currentUserEmail || remove.isPending}
              onClick={async () => {
                const ok = await confirm({
                  title: `Delete ${user.username ?? user.email}?`,
                  description: 'This account and its sessions are removed immediately.',
                  confirmLabel: 'Delete',
                })
                if (ok) remove.mutate(user.email, { onError })
              }}
            >
              Delete
            </Button>
          </div>
        ))}
      </div>
      <form onSubmit={submit} className="flex flex-wrap items-end gap-2 border-t p-4">
        <div className="flex flex-col gap-2">
          <Label htmlFor="new-email">Email</Label>
          <Input
            id="new-email"
            type="email"
            autoComplete="off"
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="new-username">Username (optional)</Label>
          <Input
            id="new-username"
            autoComplete="off"
            pattern="[A-Za-z0-9._\-]{3,32}"
            title="3-32 letters, digits, '.', '_' or '-'"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="new-user-password">Password</Label>
          <Input
            id="new-user-password"
            type="password"
            autoComplete="new-password"
            required
            minLength={8}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="new-user-role">Role</Label>
          <select
            id="new-user-role"
            className="h-8 border border-input bg-transparent px-2 text-sm"
            value={role}
            onChange={(e) => setRole(e.target.value as UserRole)}
          >
            <option value="viewer">viewer</option>
            <option value="admin">admin</option>
          </select>
        </div>
        <label className="flex w-full items-center gap-2 text-sm">
          <Checkbox checked={requireChange} onCheckedChange={setRequireChange} />
          Require a new password at first sign-in
        </label>
        <Button type="submit" disabled={create.isPending}>
          Add user
        </Button>
      </form>
    </section>
  )
}

export function AccountSettings() {
  const user = useAuthStatus().data?.user
  if (!user) return null
  return (
    <>
      <ProfileForm key={`${user.email}:${user.username}`} user={user} />
      <ChangePasswordForm />
      <SessionsPanel />
      {user.role === 'admin' && <UsersPanel currentUserEmail={user.email} />}
      {user.role === 'admin' && <SecurityLog />}
    </>
  )
}
