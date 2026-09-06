import { Navigate, useNavigate, useParams } from 'react-router-dom'
import { SteamIdListEditor } from '@/components/instance/SteamIdListEditor'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

const LIST_TABS = ['admin', 'banned', 'permitted'] as const
type ListTab = (typeof LIST_TABS)[number]

function isListTab(value: string | undefined): value is ListTab {
  return LIST_TABS.some((tab) => tab === value)
}

export function AccessListsTab({ name, path }: { name: string; path: string[] }) {
  const navigate = useNavigate()
  const { game } = useParams<{ game?: string }>()
  const [tab, ...rest] = path
  const basePath = `${game === 'valheim' ? `/instances/valheim/${name}` : `/instances/${name}`}/lists`

  if (!isListTab(tab) || rest.length > 0) return <Navigate to={`${basePath}/admin`} replace />

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        Valheim reads these directly from the world save directory — no restart required.
      </p>
      <Tabs value={tab} onValueChange={(value) => navigate(`${basePath}/${value}`)}>
        <TabsList variant="line">
          <TabsTrigger value="admin">Admins</TabsTrigger>
          <TabsTrigger value="banned">Banned</TabsTrigger>
          <TabsTrigger value="permitted">Permitted builders</TabsTrigger>
        </TabsList>
        {tab === 'admin' && (
          <TabsContent value="admin">
            <SteamIdListEditor name={name} kind="admin" />
          </TabsContent>
        )}
        {tab === 'banned' && (
          <TabsContent value="banned">
            <SteamIdListEditor name={name} kind="banned" />
          </TabsContent>
        )}
        {tab === 'permitted' && (
          <TabsContent value="permitted">
            <SteamIdListEditor name={name} kind="permitted" />
          </TabsContent>
        )}
      </Tabs>
    </div>
  )
}
