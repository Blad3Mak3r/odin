import { Monitor, Moon, Sun } from 'lucide-react'
import { useTheme } from 'next-themes'
import { cn } from '@/lib/utils'

const OPTIONS = [
  { value: 'light', label: 'Light', icon: Sun },
  { value: 'system', label: 'System', icon: Monitor },
  { value: 'dark', label: 'Dark', icon: Moon },
] as const

export function ThemeToggle() {
  const { theme, setTheme } = useTheme()

  return (
    <div className="flex items-center gap-1 rounded-md border bg-sidebar p-1">
      {OPTIONS.map(({ value, label, icon: Icon }) => (
        <button
          key={value}
          type="button"
          title={label}
          aria-label={label}
          onClick={() => setTheme(value)}
          className={cn(
            'flex flex-1 items-center justify-center rounded-sm py-1.5 text-sidebar-foreground/70 transition-colors hover:text-sidebar-foreground',
            theme === value && 'bg-sidebar-accent text-sidebar-accent-foreground',
          )}
        >
          <Icon className="size-4" />
        </button>
      ))}
    </div>
  )
}
