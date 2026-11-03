import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { RouteErrorBoundary } from './RouteErrorBoundary'

// Tells React that `act` is in use, so it applies updates synchronously.
;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

function Boom(): never {
  throw new Error('chunk failed to load')
}

describe('RouteErrorBoundary', () => {
  let container: HTMLDivElement
  let root: Root

  const render = (resetKey: string, child: React.ReactNode) =>
    act(() => {
      root.render(<RouteErrorBoundary resetKey={resetKey}>{child}</RouteErrorBoundary>)
    })

  beforeEach(() => {
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
    // React and the boundary both log the caught error.
    vi.spyOn(console, 'error').mockImplementation(() => {})
  })

  afterEach(() => {
    act(() => root.unmount())
    container.remove()
    vi.restoreAllMocks()
  })

  it('shows the error in place of a page that crashes', () => {
    render('/jobs', <Boom />)

    expect(container.textContent).toContain('This page crashed')
    expect(container.textContent).toContain('chunk failed to load')
  })

  it('renders its children when nothing throws', () => {
    render('/jobs', <p>All good</p>)

    expect(container.textContent).toBe('All good')
  })

  it('stays on the error while the page is the same', () => {
    render('/jobs', <Boom />)
    render('/jobs', <p>All good</p>)

    expect(container.textContent).toContain('This page crashed')
  })

  it('recovers when the user navigates to another page', () => {
    render('/jobs', <Boom />)
    render('/settings', <p>Settings</p>)

    expect(container.textContent).toBe('Settings')
  })
})
