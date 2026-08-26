import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B"
  const units = ["B", "KB", "MB", "GB", "TB"]
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  return `${(bytes / 1024 ** exponent).toFixed(1)} ${units[exponent]}`
}

const RELATIVE_TIME_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 31536000],
  ["month", 2592000],
  ["week", 604800],
  ["day", 86400],
  ["hour", 3600],
  ["minute", 60],
]

const relativeTimeFormat = new Intl.RelativeTimeFormat("en", { numeric: "auto" })

export function formatRelativeTime(iso: string): string {
  const diffSeconds = Math.round((new Date(iso).getTime() - Date.now()) / 1000)
  const abs = Math.abs(diffSeconds)

  for (const [unit, secondsInUnit] of RELATIVE_TIME_UNITS) {
    if (abs >= secondsInUnit) {
      return relativeTimeFormat.format(Math.round(diffSeconds / secondsInUnit), unit)
    }
  }
  return relativeTimeFormat.format(diffSeconds, "second")
}
