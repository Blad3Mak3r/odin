import { Dialog as DialogPrimitive } from '@base-ui/react/dialog'
import { Blocks, LayoutDashboard, Menu, Server, X } from 'lucide-react'
import { type ReactNode, useState } from 'react'
import { NavLink } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { useLiveSocket } from '@/hooks/useLiveSocket'
import { useVersion } from '@/lib/queries'
import { cn } from '@/lib/utils'
import { ThemeToggle } from './ThemeToggle'

const NAV_ITEMS = [
  { to: '/', label: 'Dashboard', icon: LayoutDashboard, end: true },
  { to: '/instances', label: 'Instances', icon: Server, end: false },
  { to: '/mods', label: 'Mods', icon: Blocks, end: false },
]

function SidebarNav({ onNavigate }: { onNavigate?: () => void }) {
  const version = useVersion()

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
        {version.data && (
          <span className="text-xs text-muted-foreground">v{version.data.version}</span>
        )}
      </div>
    </>
  )
}

export function AppShell({ children }: { children: ReactNode }) {
  useLiveSocket()
  const [mobileNavOpen, setMobileNavOpen] = useState(false)

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
            <SidebarNav onNavigate={() => setMobileNavOpen(false)} />
          </DialogPrimitive.Popup>
        </DialogPrimitive.Portal>
      </DialogPrimitive.Root>

      <aside className="hidden w-56 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground md:flex">
        <SidebarNav />
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-5xl px-4 py-6 sm:px-6 sm:py-8">{children}</div>
      </main>
    </div>
  )
}
