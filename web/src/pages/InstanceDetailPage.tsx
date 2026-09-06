import { useState } from 'react'
import { Navigate, useNavigate, useParams } from 'react-router-dom'
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

const INSTANCE_TABS = ['logs', 'config', 'mods', 'lists', 'backups', 'resources', 'players'] as const
type InstanceTab = (typeof INSTANCE_TABS)[number]

function isInstanceTab(value: string | undefined): value is InstanceTab {
  return INSTANCE_TABS.some((tab) => tab === value)
}

export function InstanceDetailPage() {
  const { game, name, '*': tabPath } = useParams<{ game?: string; name: string; '*': string }>()
  const navigate = useNavigate()
  const [deleteOpen, setDeleteOpen] = useState(false)
  const instance = useInstance(name ?? '')

  if (!name) return null

  const basePath = game === 'valheim' ? `/instances/valheim/${name}` : `/instances/${name}`
  const segments = tabPath?.split('/').filter(Boolean) ?? []
  const [tab, ...nestedPath] = segments

  if (!isInstanceTab(tab)) return <Navigate to={`${basePath}/logs`} replace />
  if (tab !== 'mods' && tab !== 'lists' && nestedPath.length > 0) {
    return <Navigate to={`${basePath}/logs`} replace />
  }

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

      <Tabs
        value={tab}
        onValueChange={(value) =>
          navigate(
            value === 'mods'
              ? `${basePath}/mods/installed`
              : value === 'lists'
                ? `${basePath}/lists/admin`
                : `${basePath}/${value}`,
          )
        }
      >
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
        {tab === 'logs' && (
          <TabsContent value="logs">
            <LogsTab name={name} />
          </TabsContent>
        )}
        {tab === 'config' && (
          <TabsContent value="config">
            <ConfigTab name={name} />
          </TabsContent>
        )}
        {tab === 'mods' && (
          <TabsContent value="mods">
            <ModsTab name={name} running={instance.data?.running ?? false} path={nestedPath} />
          </TabsContent>
        )}
        {tab === 'lists' && (
          <TabsContent value="lists">
            <AccessListsTab name={name} path={nestedPath} />
          </TabsContent>
        )}
        {tab === 'backups' && (
          <TabsContent value="backups">
            <BackupsTab name={name} running={instance.data?.running ?? false} />
          </TabsContent>
        )}
        {tab === 'resources' && (
          <TabsContent value="resources">
            <ResourcesTab name={name} running={instance.data?.running ?? false} />
          </TabsContent>
        )}
        {tab === 'players' && (
          <TabsContent value="players">
            <PlayersTab name={name} running={instance.data?.running ?? false} />
          </TabsContent>
        )}
      </Tabs>
    </div>
  )
}
