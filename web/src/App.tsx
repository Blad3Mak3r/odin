import { Loader2 } from 'lucide-react'
import { lazy, Suspense } from 'react'
import { Route, Routes } from 'react-router-dom'
import { AppShell } from '@/components/layout/AppShell'

const DashboardPage = lazy(() =>
  import('@/pages/DashboardPage').then((m) => ({ default: m.DashboardPage })),
)
const InstancesPage = lazy(() =>
  import('@/pages/InstancesPage').then((m) => ({ default: m.InstancesPage })),
)
const InstanceDetailPage = lazy(() =>
  import('@/pages/InstanceDetailPage').then((m) => ({ default: m.InstanceDetailPage })),
)
const GlobalModsPage = lazy(() =>
  import('@/pages/GlobalModsPage').then((m) => ({ default: m.GlobalModsPage })),
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
          <Route path="/instances" element={<InstancesPage />} />
          <Route path="/instances/:name" element={<InstanceDetailPage />} />
          <Route path="/mods" element={<GlobalModsPage />} />
        </Routes>
      </Suspense>
    </AppShell>
  )
}

export default App
