import sevenDaysBanner from '@/assets/game-banners/7d2d.webp'
import palworldBanner from '@/assets/game-banners/palworld.webp'
import rustBanner from '@/assets/game-banners/rust.webp'
import valheimBanner from '@/assets/game-banners/valheim.webp'
import type { GameId } from './types'

const GAME_LABELS: Record<GameId, string> = {
  valheim: 'Valheim',
  rust: 'Rust',
  palworld: 'Palworld',
  '7d2d': '7 Days to Die',
}

export const GAME_DESCRIPTIONS: Record<GameId, string> = {
  valheim: 'Build, explore, and survive with a dedicated Valheim server.',
  rust: 'Host and manage your Rust dedicated servers.',
  palworld: 'Operate your Palworld dedicated server from one place.',
  '7d2d': 'Run and manage your 7 Days to Die dedicated servers.',
}

export const GAME_BANNERS: Record<GameId, string> = {
  valheim: valheimBanner,
  rust: rustBanner,
  palworld: palworldBanner,
  '7d2d': sevenDaysBanner,
}

export function gameLabel(game: GameId) {
  return GAME_LABELS[game]
}
