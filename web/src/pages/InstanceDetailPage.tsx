import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { AccessListsTab } from '@/components/instance/AccessListsTab'
import { BackupsTab } from '@/components/instance/BackupsTab'
import { ConfigTab } from '@/components/instance/ConfigTab'
import { DeleteInstanceDialog } from '@/components/instance/DeleteInstanceDialog'
import { InstanceHeader } from '@/components/instance/InstanceHeader'
import { LogsTab } from '@/components/instance/LogsTab'
import { ModsTab } from '@/components/instance/ModsTab'
import { PlayersTab } from '@/components/instance/PlayersTab'
import { ResourcesTab } from '@/components/instance/ResourcesTab'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useInstance } from '@/lib/queries'

const TABS = ['logs', 'config', 'mods', 'lists', 'backups', 'resources', 'players'] as const
type Tab = (typeof TABS)[number]

export function InstanceDetailPage() {
  const { name } = useParams<{ name: string }>()
  const navigate = useNavigate()
  const [tab, setTab] = useState<Tab>('logs')
  const [deleteOpen, setDeleteOpen] = useState(false)
  const instance = useInstance(name ?? '')

  if (!name) return null

  return (
    <div className="flex flex-col gap-6">
      <DeleteInstanceDialog
        name={name}
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        onDeleted={() => navigate('/instances')}
      />
      <InstanceHeader
        instance={instance.data}
        loading={instance.isLoading}
        onDelete={() => setDeleteOpen(true)}
      />

      <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)}>
        <div className="overflow-x-auto">
          <TabsList className="w-max">
            <TabsTrigger value="logs">Logs</TabsTrigger>
            <TabsTrigger value="config">Config</TabsTrigger>
            <TabsTrigger value="mods">Mods</TabsTrigger>
            <TabsTrigger value="lists">Access lists</TabsTrigger>
            <TabsTrigger value="backups">Backups</TabsTrigger>
            <TabsTrigger value="resources">Resources</TabsTrigger>
            <TabsTrigger value="players">Players</TabsTrigger>
          </TabsList>
        </div>
        <TabsContent value="logs">
          <LogsTab name={name} />
        </TabsContent>
        <TabsContent value="config">
          <ConfigTab name={name} />
        </TabsContent>
        <TabsContent value="mods">
          <ModsTab name={name} />
        </TabsContent>
        <TabsContent value="lists">
          <AccessListsTab name={name} />
        </TabsContent>
        <TabsContent value="backups">
          <BackupsTab name={name} running={instance.data?.running ?? false} />
        </TabsContent>
        <TabsContent value="resources">
          <ResourcesTab name={name} running={instance.data?.running ?? false} />
        </TabsContent>
        <TabsContent value="players">
          <PlayersTab name={name} running={instance.data?.running ?? false} />
        </TabsContent>
      </Tabs>
    </div>
  )
}
