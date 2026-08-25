import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { toast } from 'sonner'
import { AccessListsTab } from '@/components/instance/AccessListsTab'
import { ConfigTab } from '@/components/instance/ConfigTab'
import { ConsoleTab } from '@/components/instance/ConsoleTab'
import { InstanceHeader } from '@/components/instance/InstanceHeader'
import { LogsTab } from '@/components/instance/LogsTab'
import { ModsTab } from '@/components/instance/ModsTab'
import { ResourcesTab } from '@/components/instance/ResourcesTab'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useDeleteInstance, useInstance } from '@/lib/queries'

const TABS = ['console', 'logs', 'config', 'mods', 'lists', 'resources'] as const
type Tab = (typeof TABS)[number]

export function InstanceDetailPage() {
  const { name } = useParams<{ name: string }>()
  const navigate = useNavigate()
  const [tab, setTab] = useState<Tab>('console')
  const instance = useInstance(name ?? '')
  const deleteInstance = useDeleteInstance()

  if (!name) return null

  const handleDelete = () => {
    if (!confirm(`Permanently delete '${name}'? This removes its world saves, config, and mods.`)) {
      return
    }
    deleteInstance.mutate(
      { name, keepBackups: false },
      {
        onSuccess: () => {
          toast.success(`Instance '${name}' deleted`)
          navigate('/instances')
        },
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <div className="flex flex-col gap-6">
      <InstanceHeader
        instance={instance.data}
        loading={instance.isLoading}
        onDelete={handleDelete}
      />

      <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)}>
        <div className="overflow-x-auto">
          <TabsList className="w-max">
            <TabsTrigger value="console">Console</TabsTrigger>
            <TabsTrigger value="logs">Logs</TabsTrigger>
            <TabsTrigger value="config">Config</TabsTrigger>
            <TabsTrigger value="mods">Mods</TabsTrigger>
            <TabsTrigger value="lists">Access lists</TabsTrigger>
            <TabsTrigger value="resources">Resources</TabsTrigger>
          </TabsList>
        </div>
        <TabsContent value="console">
          <ConsoleTab name={name} />
        </TabsContent>
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
        <TabsContent value="resources">
          <ResourcesTab name={name} running={instance.data?.running ?? false} />
        </TabsContent>
      </Tabs>
    </div>
  )
}
