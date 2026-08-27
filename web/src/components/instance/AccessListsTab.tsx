import { SteamIdListEditor } from '@/components/instance/SteamIdListEditor'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

export function AccessListsTab({ name }: { name: string }) {
  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        Valheim reads these directly from the world save directory — no restart required.
      </p>
      <Tabs defaultValue="admin">
        <TabsList variant="line">
          <TabsTrigger value="admin">Admins</TabsTrigger>
          <TabsTrigger value="banned">Banned</TabsTrigger>
          <TabsTrigger value="permitted">Permitted builders</TabsTrigger>
        </TabsList>
        <TabsContent value="admin">
          <SteamIdListEditor name={name} kind="admin" />
        </TabsContent>
        <TabsContent value="banned">
          <SteamIdListEditor name={name} kind="banned" />
        </TabsContent>
        <TabsContent value="permitted">
          <SteamIdListEditor name={name} kind="permitted" />
        </TabsContent>
      </Tabs>
    </div>
  )
}
