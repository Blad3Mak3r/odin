import sevenDaysBanner from '@/assets/game-banners/7d2d.webp'
import arkAsaBanner from '@/assets/game-banners/ark-asa.webp'
import cs2Banner from '@/assets/game-banners/cs2.webp'
import palworldBanner from '@/assets/game-banners/palworld.webp'
import rustBanner from '@/assets/game-banners/rust.webp'
import valheimBanner from '@/assets/game-banners/valheim.webp'
import type { GameId } from './types'

const GAME_LABELS: Record<GameId, string> = {
  'ark-asa': 'ARK: Survival Ascended',
  valheim: 'Valheim',
  rust: 'Rust',
  palworld: 'Palworld',
  '7d2d': '7 Days to Die',
  cs2: 'Counter-Strike 2',
}

export const GAME_DESCRIPTIONS: Record<GameId, string> = {
  'ark-asa': 'Host ARK: Survival Ascended with isolated or shared game files.',
  valheim: 'Build, explore, and survive with a dedicated Valheim server.',
  rust: 'Host and manage your Rust dedicated servers.',
  palworld: 'Operate your Palworld dedicated server from one place.',
  '7d2d': 'Run and manage your 7 Days to Die dedicated servers.',
  cs2: 'Host Counter-Strike 2 with isolated or shared game files.',
}

export const GAME_BANNERS: Record<GameId, string> = {
  'ark-asa': arkAsaBanner,
  valheim: valheimBanner,
  rust: rustBanner,
  palworld: palworldBanner,
  '7d2d': sevenDaysBanner,
  cs2: cs2Banner,
}

export function gameLabel(game: GameId) {
  return GAME_LABELS[game]
}
