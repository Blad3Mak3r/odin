import { AlertCircle } from 'lucide-react'
import { Component, type ErrorInfo, type ReactNode } from 'react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'

interface Props {
  // Changing this clears a caught error, so navigating to another page gets a
  // fresh start instead of the previous page's error.
  resetKey: string
  children: ReactNode
}

interface State {
  error: Error | null
  resetKey: string
}

// Keeps a render crash (or a page chunk that failed to load after a release)
// inside the page area, leaving the navigation usable.
export class RouteErrorBoundary extends Component<Props, State> {
  state: State = { error: null, resetKey: this.props.resetKey }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error }
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.resetKey === state.resetKey ? null : { error: null, resetKey: props.resetKey }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('Page crashed', error, info.componentStack)
  }

  render() {
    const { error } = this.state
    if (!error) return this.props.children
    return (
      <Alert variant="destructive">
        <AlertCircle />
        <AlertTitle>This page crashed</AlertTitle>
        <AlertDescription>
          <p>{error.message}</p>
          <Button variant="outline" size="sm" onClick={() => window.location.reload()}>
            Reload
          </Button>
        </AlertDescription>
      </Alert>
    )
  }
}
