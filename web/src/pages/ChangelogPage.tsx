import { PageHeader } from '@/components/PageHeader'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import { Skeleton } from '@/components/ui/skeleton'
import { useChangelog } from '@/lib/queries'

const currentVersion = import.meta.env.VITE_ODIN_VERSION

export function ChangelogPage() {
  const changelog = useChangelog()

  return (
    <div className="flex max-w-[70ch] flex-col gap-8">
      <PageHeader
        title="Changelog"
        description="A record of the features, improvements, and fixes included in each Odin release."
      />

      {changelog.isError && <QueryError error={changelog.error} />}

      {changelog.isLoading && (
        <div className="flex flex-col gap-8" aria-label="Loading changelog">
          {[0, 1].map((item) => (
            <div key={item} className="flex flex-col gap-4">
              <div className="flex items-center gap-3">
                <Skeleton className="h-7 w-20" />
                <Skeleton className="h-5 w-24" />
              </div>
              <Skeleton className="h-4 w-28" />
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-4/5" />
            </div>
          ))}
        </div>
      )}

      {changelog.data && (
        <div className="flex flex-col">
          {changelog.data.map((release, index) => {
            const isCurrent = release.version === currentVersion
            return (
              <article key={release.version} className="flex flex-col gap-5 py-8 first:pt-0">
                <header className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                  <div className="flex items-center gap-2">
                    <h2 className="text-xl font-semibold tracking-tight">v{release.version}</h2>
                    {isCurrent && <Badge variant="secondary">Current</Badge>}
                  </div>
                  {release.date && (
                    <time dateTime={release.date} className="text-sm tabular-nums text-muted-foreground">
                      {release.date}
                    </time>
                  )}
                </header>

                <div className="flex flex-col gap-6">
                  {release.sections.map((section) => (
                    <section key={section.title} className="flex flex-col gap-2">
                      <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
                        {section.title}
                      </h3>
                      <ul className="flex list-disc flex-col gap-2 pl-5 text-sm leading-6 marker:text-muted-foreground">
                        {section.changes.map((change) => (
                          <li key={change}>{change}</li>
                        ))}
                      </ul>
                    </section>
                  ))}
                </div>

                {index < changelog.data.length - 1 && <Separator className="mt-3" />}
              </article>
            )
          })}
        </div>
      )}
    </div>
  )
}
