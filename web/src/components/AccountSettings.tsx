import { type FormEvent, useState } from 'react'
import { toast } from 'sonner'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  useAuthStatus,
  useChangePassword,
  useCreateUser,
  useDeleteUser,
  useUpdateUser,
  useUsers,
} from '@/lib/queries'
import type { UserRole } from '@/lib/types'

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

  const onError = (e: Error) => toast.error(e.message)

  const submit = (event: FormEvent) => {
    event.preventDefault()
    create.mutate(
      { email, username: username.trim() || undefined, password, role },
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

  const resetPassword = (email: string, name: string) => {
    const value = window.prompt(`New password for ${name} (at least 8 characters)`)
    if (!value) return
    update.mutate(
      { email, password: value },
      { onSuccess: () => toast.success(`Password reset for ${name}`), onError },
    )
  }

  return (
    <section className="max-w-2xl border">
      {dialog}
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
            </span>
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
              onClick={() => resetPassword(user.email, user.username ?? user.email)}
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
      <ChangePasswordForm />
      {user.role === 'admin' && <UsersPanel currentUserEmail={user.email} />}
    </>
  )
}
