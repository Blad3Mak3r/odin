export function QueryError({ error }: { error: unknown }) {
  const message = error instanceof Error ? error.message : 'Something went wrong.'
  return <p className="text-sm text-destructive">{message}</p>
}
