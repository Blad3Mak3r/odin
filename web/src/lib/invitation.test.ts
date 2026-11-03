import { describe, expect, it } from 'vitest'
import { readInvitationToken } from './invitation'

const TOKEN = 'ab'.repeat(32)

describe('readInvitationToken', () => {
  it('reads a well-formed fragment', () => {
    expect(readInvitationToken(`#token=${TOKEN}`)).toBe(TOKEN)
  })

  it('lowercases the token', () => {
    expect(readInvitationToken(`#token=${TOKEN.toUpperCase()}`)).toBe(TOKEN)
  })

  it('rejects a missing, short or padded token', () => {
    expect(readInvitationToken('')).toBeNull()
    expect(readInvitationToken('#token=')).toBeNull()
    expect(readInvitationToken(`#token=${TOKEN.slice(1)}`)).toBeNull()
    expect(readInvitationToken(`#token=${TOKEN}&x=1`)).toBeNull()
    expect(readInvitationToken(`#other=${TOKEN}`)).toBeNull()
  })
})
