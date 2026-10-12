import { NavLink } from 'react-router-dom'
import { cn } from '@/lib/utils'

export type RouteTab = { label: string; to: string; end?: boolean }

export function RouteTabs({ label, tabs }: { label: string; tabs: RouteTab[] }) {
  return (
    <nav aria-label={label} className="overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden max-md:[mask-image:linear-gradient(to_right,black_calc(100%-1.5rem),transparent)]">
      <div className="flex w-max min-w-full border-b text-muted-foreground after:w-6 after:shrink-0 md:after:hidden">
        {tabs.map((tab) => (
          <NavLink
            key={tab.to}
            to={tab.to}
            end={tab.end ?? false}
            className={({ isActive }) => cn(
              '-mb-px inline-flex h-11 items-center border-b-2 px-3 text-sm font-medium whitespace-nowrap transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring md:h-9',
              isActive ? 'border-foreground text-foreground' : 'border-transparent hover:text-foreground',
            )}
          >
            {tab.label}
          </NavLink>
        ))}
      </div>
    </nav>
  )
}
