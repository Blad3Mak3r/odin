import { Navigate, useNavigate } from 'react-router-dom'
import { SteamIdListEditor } from '@/components/instance/SteamIdListEditor'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

const LIST_TABS = ['admin', 'banned', 'permitted'] as const
type ListTab = (typeof LIST_TABS)[number]

function isListTab(value: string | undefined): value is ListTab {
  return LIST_TABS.some((tab) => tab === value)
}

export function AccessListsTab({ id, path }: { id: string; path: string[] }) {
  const navigate = useNavigate()
  const [tab, ...rest] = path
  const basePath = `/instance/${id}/lists`

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
            <SteamIdListEditor id={id} kind="admin" />
          </TabsContent>
        )}
        {tab === 'banned' && (
          <TabsContent value="banned">
            <SteamIdListEditor id={id} kind="banned" />
          </TabsContent>
        )}
        {tab === 'permitted' && (
          <TabsContent value="permitted">
            <SteamIdListEditor id={id} kind="permitted" />
          </TabsContent>
        )}
      </Tabs>
    </div>
  )
}
