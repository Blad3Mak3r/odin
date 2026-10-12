import { memo } from 'react'
import { parseAnsi } from '@/lib/ansi'

/** Renders a console line, honoring its ANSI color and text-style escapes. */
export const AnsiText = memo(function AnsiText({ text }: { text: string }) {
  return parseAnsi(text).map((segment, index) =>
    segment.style ? <span key={index} style={segment.style}>{segment.text}</span> : segment.text,
  )
})
