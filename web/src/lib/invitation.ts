/** Where the emailed invitation link lands; the server builds the same path. */
export const ACCEPT_INVITE_PATH = '/accept-invite'

/** The token in a link's fragment (`#token=<64 hex>`), or null when it is missing or malformed. */
export function readInvitationToken(hash: string): string | null {
  const match = /^#token=([0-9a-f]{64})$/i.exec(hash)
  return match ? match[1].toLowerCase() : null
}
