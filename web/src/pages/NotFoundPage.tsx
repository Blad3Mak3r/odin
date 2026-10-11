import { Link } from 'react-router-dom'
import { Button } from '@/components/ui/button'

export function NotFoundPage() {
  return (
    <div className="flex flex-col items-start gap-4 py-12">
      <div>
        <p className="text-sm font-medium text-muted-foreground">404</p>
        <h1 className="text-2xl font-semibold tracking-tight">Page not found</h1>
        <p className="mt-1 text-sm text-muted-foreground">This page no longer exists or the address is invalid.</p>
      </div>
      <Button render={<Link to="/" />}>Go to dashboard</Button>
    </div>
  )
}
