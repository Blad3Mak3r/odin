import sevenDaysBanner from '@/assets/game-banners/7d2d.webp'
import palworldBanner from '@/assets/game-banners/palworld.webp'
import runescapeDragonwildsBanner from '@/assets/game-banners/runescape-dragonwilds.webp'
import rustBanner from '@/assets/game-banners/rust.webp'
import valheimBanner from '@/assets/game-banners/valheim.webp'
import vrisingBanner from '@/assets/game-banners/vrising.webp'
import type { GameId } from './types'

const GAME_LABELS: Record<GameId, string> = {
  valheim: 'Valheim',
  rust: 'Rust',
  vrising: 'V Rising',
  palworld: 'Palworld',
  'runescape-dragonwilds': 'RuneScape: Dragonwilds',
  '7d2d': '7 Days to Die',
}

export const GAME_DESCRIPTIONS: Record<GameId, string> = {
  valheim: 'Build, explore, and survive with a dedicated Valheim server.',
  rust: 'Host and manage your Rust dedicated servers.',
  vrising: 'Run your V Rising server and manage its world.',
  palworld: 'Operate your Palworld dedicated server from one place.',
  'runescape-dragonwilds': 'Host your RuneScape: Dragonwilds server.',
  '7d2d': 'Run and manage your 7 Days to Die dedicated servers.',
}

export const GAME_BANNERS: Record<GameId, string> = {
  valheim: valheimBanner,
  rust: rustBanner,
  vrising: vrisingBanner,
  palworld: palworldBanner,
  'runescape-dragonwilds': runescapeDragonwildsBanner,
  '7d2d': sevenDaysBanner,
}

export function gameLabel(game: GameId) {
  return GAME_LABELS[game]
}
