import { Loader2 } from 'lucide-react'
import { lazy, Suspense } from 'react'
import { Route, Routes } from 'react-router-dom'
import { AppShell } from '@/components/layout/AppShell'

const DashboardPage = lazy(() =>
  import('@/pages/DashboardPage').then((m) => ({ default: m.DashboardPage })),
)
const MultiGameInstancesPage = lazy(() =>
  import('@/pages/MultiGameInstancesPage').then((m) => ({ default: m.MultiGameInstancesPage })),
)
const ManagedInstanceDetailPage = lazy(() =>
  import('@/pages/ManagedInstanceDetailPage').then((m) => ({ default: m.ManagedInstanceDetailPage })),
)
const InstanceDetailPage = lazy(() =>
  import('@/pages/InstanceDetailPage').then((m) => ({ default: m.InstanceDetailPage })),
)
const GlobalModsPage = lazy(() =>
  import('@/pages/GlobalModsPage').then((m) => ({ default: m.GlobalModsPage })),
)
const JobsPage = lazy(() => import('@/pages/JobsPage').then((m) => ({ default: m.JobsPage })))
const WebhooksPage = lazy(() =>
  import('@/pages/WebhooksPage').then((m) => ({ default: m.WebhooksPage })),
)
const SettingsPage = lazy(() =>
  import('@/pages/SettingsPage').then((m) => ({ default: m.SettingsPage })),
)
const ChangelogPage = lazy(() =>
  import('@/pages/ChangelogPage').then((m) => ({ default: m.ChangelogPage })),
)

function RouteFallback() {
  return (
    <div className="flex justify-center py-12">
      <Loader2 className="size-6 animate-spin text-muted-foreground" />
    </div>
  )
}

function App() {
  return (
    <AppShell>
      <Suspense fallback={<RouteFallback />}>
        <Routes>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/instances" element={<MultiGameInstancesPage />} />
          <Route path="/instances/:game/:name" element={<ManagedInstanceDetailPage />} />
          <Route path="/instances/:name/*" element={<InstanceDetailPage />} />
          <Route path="/mods/*" element={<GlobalModsPage />} />
          <Route path="/jobs" element={<JobsPage />} />
          <Route path="/webhooks" element={<WebhooksPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/changelog" element={<ChangelogPage />} />
        </Routes>
      </Suspense>
    </AppShell>
  )
}

export default App
