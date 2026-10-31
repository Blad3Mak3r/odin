import { Loader2, Sparkles, TriangleAlert, X } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { UpdateProgressDialog } from '@/components/UpdateProgressDialog'
import { Button } from '@/components/ui/button'
import { gameLabel } from '@/lib/games'
import { useIsAdmin, useSystemUpdate, useVersion } from '@/lib/queries'

const DISMISSED_KEY = 'odin-update-dismissed-version'

function readDismissedVersion(): string | null {
  try {
    return localStorage.getItem(DISMISSED_KEY)
  } catch {
    return null
  }
}

function writeDismissedVersion(version: string) {
  try {
    localStorage.setItem(DISMISSED_KEY, version)
  } catch {
    // Private browsing / blocked storage: the banner just reappears next
    // load, which is a harmless degradation.
  }
}

/// Persistent bar for Odin updates: a release notice can be dismissed per
/// version, while running instances left on an older supervisor stay visible
/// until they are restarted.
export function UpdateBanner() {
  const isAdmin = useIsAdmin()
  const version = useVersion()
  const systemUpdate = useSystemUpdate()
  const { confirm, dialog } = useConfirmDialog()
  const [dismissed, setDismissed] = useState<string | null>(() => readDismissedVersion())
  // Version the dashboard was running when the update started; set while the
  // progress dialog follows the restart.
  const [updatingFrom, setUpdatingFrom] = useState<string | null>(null)

  const latest = version.data?.update_available ? version.data.latest_version : null
  const availableRelease = latest && latest !== dismissed ? latest : null
  const outdated = version.data?.outdated_instances ?? []
  const canInstall = Boolean(availableRelease && version.data?.system_update_supported)
  const updating = systemUpdate.isPending || Boolean(version.data?.system_update_running)
  const startUpdate = async () => {
    if (!(await confirm({
      title: 'Update Odin?',
      description: 'The latest Odin package will be installed and the dashboard will reload automatically once Odin has restarted. Running game servers will stay online, but restart them afterward to use the new supervisor.',
      confirmLabel: 'Update now',
    }))) return
    const startVersion = version.data?.current_version ?? null
    systemUpdate.mutate(undefined, {
      onSuccess: () => setUpdatingFrom(startVersion),
      onError: (error) => toast.error(error.message),
    })
  }
  const progress = updatingFrom && (
    <UpdateProgressDialog startVersion={updatingFrom} onClose={() => setUpdatingFrom(null)} />
  )
  if (!isAdmin || (!availableRelease && outdated.length === 0)) {
    return progress || null
  }

  return (
    <>
      {progress}
      <div
        role="status"
        aria-live="polite"
        className="flex items-start justify-between gap-3 border-b bg-accent px-4 py-2 text-sm text-accent-foreground"
      >
        <div className="flex min-w-0 flex-col gap-1">
          {outdated.length > 0 && (
            <span className="flex items-start gap-2">
              <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
              <span>
                {outdated.length} running{' '}
                {outdated.length === 1 ? 'instance still uses' : 'instances still use'} an older
                Odin supervisor. Restart {outdated.map(({ game, name }) => `${gameLabel(game)} / ${name}`).join(', ')} to finish applying the update.
              </span>
            </span>
          )}
          {availableRelease && (
            <span className="flex items-center gap-2">
              <Sparkles className="size-4 shrink-0" aria-hidden="true" />
              Odin {availableRelease} is available.
              {version.data?.latest_release_url && (
                <a
                  href={version.data.latest_release_url}
                  target="_blank"
                  rel="noreferrer"
                  className="underline underline-offset-2"
                >
                  View release
                </a>
              )}
              {canInstall && (
                <Button variant="outline" size="sm" disabled={updating} onClick={startUpdate}>
                  {updating && <Loader2 className="size-4 animate-spin" />}
                  {updating ? 'Updating…' : 'Update now'}
                </Button>
              )}
            </span>
          )}
        </div>
        {availableRelease && (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Dismiss update notice"
            onClick={() => {
              writeDismissedVersion(availableRelease)
              setDismissed(availableRelease)
            }}
          >
            <X />
          </Button>
        )}
        {dialog}
      </div>
    </>
  )
}
