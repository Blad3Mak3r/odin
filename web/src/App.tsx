import { Route, Routes } from 'react-router-dom'
import { AppShell } from '@/components/layout/AppShell'
import { DashboardPage } from '@/pages/DashboardPage'
import { InstanceDetailPage } from '@/pages/InstanceDetailPage'
import { InstancesPage } from '@/pages/InstancesPage'

function App() {
  return (
    <AppShell>
      <Routes>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/instances" element={<InstancesPage />} />
        <Route path="/instances/:name" element={<InstanceDetailPage />} />
      </Routes>
    </AppShell>
  )
}

export default App
