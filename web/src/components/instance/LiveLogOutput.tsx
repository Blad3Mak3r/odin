import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'

const FOLLOW_THRESHOLD_PX = 8

type LiveLogOutputProps = {
  lines: string[]
  emptyMessage: string
}

/**
 * A scrollable console output that follows new entries until the user scrolls
 * away from the bottom. This keeps live output readable without repeatedly
 * pulling someone away while they inspect an older line.
 */
export function LiveLogOutput({ lines, emptyMessage }: LiveLogOutputProps) {
  const outputRef = useRef<HTMLPreElement>(null)
  const shouldFollowRef = useRef(true)
  const [following, setFollowing] = useState(true)

  useEffect(() => {
    const output = outputRef.current
    if (!output || !shouldFollowRef.current) return
    output.scrollTop = output.scrollHeight
  }, [lines])

  const updateFollowing = () => {
    const output = outputRef.current
    if (!output) return
    const atBottom = output.scrollHeight - output.scrollTop - output.clientHeight <= FOLLOW_THRESHOLD_PX
    shouldFollowRef.current = atBottom
    setFollowing(atBottom)
  }

  const resumeFollowing = () => {
    const output = outputRef.current
    if (!output) return
    shouldFollowRef.current = true
    setFollowing(true)
    output.scrollTop = output.scrollHeight
  }

  return (
    <div className="relative">
      <pre
        ref={outputRef}
        className="max-h-96 overflow-auto whitespace-pre-wrap rounded-md bg-muted p-3 text-xs"
        onScroll={updateFollowing}
      >
        {lines.join('\n') || emptyMessage}
      </pre>
      {!following && (
        <Button className="absolute right-2 bottom-2" size="sm" onClick={resumeFollowing}>
          Follow live output
        </Button>
      )}
    </div>
  )
}
