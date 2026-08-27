import { Dialog as DialogPrimitive } from '@base-ui/react/dialog'
import { Bell, Blocks, LayoutDashboard, ListChecks, Menu, Server, X } from 'lucide-react'
import { type ReactNode, useState } from 'react'
import { NavLink } from 'react-router-dom'
import { ActivityFeedPanel } from '@/components/ActivityFeedPanel'
import { Button } from '@/components/ui/button'
import { UpdateBanner } from '@/components/UpdateBanner'
import { useLiveSocket } from '@/hooks/useLiveSocket'
import { cn } from '@/lib/utils'
import { ThemeToggle } from './ThemeToggle'

const NAV_ITEMS = [
  { to: '/', label: 'Dashboard', icon: LayoutDashboard, end: true },
  { to: '/instances', label: 'Instances', icon: Server, end: false },
  { to: '/mods', label: 'Mods', icon: Blocks, end: false },
  { to: '/jobs', label: 'Jobs', icon: ListChecks, end: false },
]

function SidebarNav({
  onNavigate,
  onOpenActivity,
}: {
  onNavigate?: () => void
  onOpenActivity: () => void
}) {
  return (
    <>
      <div className="flex items-center gap-2 px-4 py-5">
        <img src="/logo.png" alt="" className="size-8 shrink-0" />
        <div>
          <span className="text-lg font-semibold tracking-tight">Odin</span>
          <p className="text-xs text-muted-foreground">Valheim server dashboard</p>
        </div>
      </div>
      <nav className="flex flex-col gap-1 px-2">
        {NAV_ITEMS.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            onClick={onNavigate}
            className={({ isActive }) =>
              cn(
                'flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium transition-colors',
                isActive
                  ? 'bg-sidebar-accent text-sidebar-accent-foreground'
                  : 'text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
              )
            }
          >
            <Icon className="size-4" />
            {label}
          </NavLink>
        ))}
      </nav>
      <div className="mt-auto flex items-center justify-between px-3 py-3">
        <ThemeToggle />
        <div className="flex items-center gap-1">
          {import.meta.env.VITE_ODIN_VERSION && (
            <span className="text-xs text-muted-foreground">v{import.meta.env.VITE_ODIN_VERSION}</span>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Open activity feed"
            onClick={onOpenActivity}
          >
            <Bell className="size-4" />
          </Button>
        </div>
      </div>
    </>
  )
}

export function AppShell({ children }: { children: ReactNode }) {
  useLiveSocket()
  const [mobileNavOpen, setMobileNavOpen] = useState(false)
  const [activityOpen, setActivityOpen] = useState(false)

  return (
    <div className="flex h-svh flex-col overflow-hidden md:flex-row">
      <DialogPrimitive.Root open={mobileNavOpen} onOpenChange={setMobileNavOpen}>
        <header className="flex shrink-0 items-center justify-between border-b bg-sidebar px-3 py-2.5 text-sidebar-foreground md:hidden">
          <span className="flex items-center gap-2">
            <img src="/logo.png" alt="" className="size-6 shrink-0" />
            <span className="text-base font-semibold tracking-tight">Odin</span>
          </span>
          <DialogPrimitive.Trigger
            render={<Button variant="ghost" size="icon-sm" aria-label="Open navigation" />}
          >
            <Menu className="size-5" />
          </DialogPrimitive.Trigger>
        </header>

        <DialogPrimitive.Portal>
          <DialogPrimitive.Backdrop className="fixed inset-0 z-50 bg-black/30 md:hidden data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0" />
          <DialogPrimitive.Popup className="fixed inset-y-0 left-0 z-50 flex h-svh w-64 max-w-[80vw] -translate-x-full flex-col border-r bg-sidebar text-sidebar-foreground outline-none transition-transform duration-200 ease-out data-open:translate-x-0 md:hidden">
            <DialogPrimitive.Title className="sr-only">Navigation</DialogPrimitive.Title>
            <div className="flex items-center justify-end px-2 pt-2">
              <DialogPrimitive.Close
                render={<Button variant="ghost" size="icon-sm" aria-label="Close navigation" />}
              >
                <X className="size-4" />
              </DialogPrimitive.Close>
            </div>
            <SidebarNav
              onNavigate={() => setMobileNavOpen(false)}
              onOpenActivity={() => setActivityOpen(true)}
            />
          </DialogPrimitive.Popup>
        </DialogPrimitive.Portal>
      </DialogPrimitive.Root>

      <aside className="hidden w-56 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground md:flex">
        <SidebarNav onOpenActivity={() => setActivityOpen(true)} />
      </aside>

      <main className="flex min-w-0 flex-1 flex-col overflow-y-auto">
        <UpdateBanner />
        <div className="mx-auto max-w-7xl px-4 py-6 sm:px-6 sm:py-8">{children}</div>
      </main>

      <ActivityFeedPanel open={activityOpen} onOpenChange={setActivityOpen} />
    </div>
  )
}
