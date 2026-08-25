import { Route, Routes } from 'react-router-dom'
import { AppShell } from '@/components/layout/AppShell'
import { DashboardPage } from '@/pages/DashboardPage'
import { GlobalModsPage } from '@/pages/GlobalModsPage'
import { InstanceDetailPage } from '@/pages/InstanceDetailPage'
import { InstancesPage } from '@/pages/InstancesPage'

function App() {
  return (
    <AppShell>
      <Routes>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/instances" element={<InstancesPage />} />
        <Route path="/instances/:name" element={<InstanceDetailPage />} />
        <Route path="/mods" element={<GlobalModsPage />} />
      </Routes>
    </AppShell>
  )
}

export default App
