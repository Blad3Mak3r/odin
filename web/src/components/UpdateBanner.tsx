import { Sparkles, X } from 'lucide-react'
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

/// Persistent bar shown when a newer Odin release is available, dismissible
/// per-version so it doesn't nag again until an even newer one ships.
export function UpdateBanner() {
  const version = useVersion()
  const [dismissed, setDismissed] = useState<string | null>(() => readDismissedVersion())

  const latest = version.data?.update_available ? version.data.latest_version : null
  if (!latest || latest === dismissed) {
    return null
  }

  return (
    <div className="flex items-center justify-between gap-3 border-b bg-accent px-4 py-2 text-sm text-accent-foreground">
      <span className="flex items-center gap-2">
        <Sparkles className="size-4 shrink-0" />
        Odin {latest} is available.
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
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Dismiss update notice"
        onClick={() => {
          writeDismissedVersion(latest)
          setDismissed(latest)
        }}
      >
        <X className="size-4" />
      </Button>
    </div>
  )
}
