export type LogEntry = {
  /** When the runtime read the line; absent for lines logged before stamping existed. */
  time?: Date
  message: string
}

// Written by the runtime: `2026-10-14T09:30:05.123Z <message>`.
const STAMPED = /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z) (.*)$/s

export function parseLogLine(line: string): LogEntry {
  const match = STAMPED.exec(line)
  if (!match) return { message: line }
  const time = new Date(match[1])
  return Number.isNaN(time.getTime()) ? { message: line } : { time, message: match[2] }
}
