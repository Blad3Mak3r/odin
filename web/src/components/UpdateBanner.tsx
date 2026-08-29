import { Sparkles, TriangleAlert, X } from 'lucide-react'
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { useVersion } from '@/lib/queries'

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
  const version = useVersion()
  const [dismissed, setDismissed] = useState<string | null>(() => readDismissedVersion())

  const latest = version.data?.update_available ? version.data.latest_version : null
  const availableRelease = latest && latest !== dismissed ? latest : null
  const outdated = version.data?.outdated_instances ?? []
  if (!availableRelease && outdated.length === 0) {
    return null
  }

  return (
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
              Odin supervisor. Restart {outdated.join(', ')} to finish applying the update.
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
    </div>
  )
}
