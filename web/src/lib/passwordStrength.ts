export interface PasswordStrength {
  /** 0 (weak) to 3 (strong). */
  score: 0 | 1 | 2 | 3
  label: string
  hint: string
}

// A hint for the person typing, not a rule: the server refuses the worst
// passwords on its own. Words attackers try first, matched anywhere in it.
const COMMON_WORDS = [
  'password',
  'passw0rd',
  'qwerty',
  'letmein',
  'welcome',
  'iloveyou',
  'admin',
  'monkey',
  'dragon',
  'football',
  'sunshine',
  'abc123',
  '123456',
  'valheim',
  'minecraft',
  'odin',
]

const LEVELS: Array<Pick<PasswordStrength, 'score' | 'label'>> = [
  { score: 0, label: 'Weak' },
  { score: 1, label: 'Fair' },
  { score: 2, label: 'Good' },
  { score: 3, label: 'Strong' },
]

function poolSize(password: string): number {
  let pool = 0
  if (/[a-z]/.test(password)) pool += 26
  if (/[A-Z]/.test(password)) pool += 26
  if (/[0-9]/.test(password)) pool += 10
  if (/[^A-Za-z0-9]/.test(password)) pool += 33
  return pool
}

// Characters that repeat or continue a run (aaa, abc, 321) add little.
function effectiveLength(password: string): number {
  let total = 0
  let previous = -1
  let previousStep = 0
  for (const char of password.toLowerCase()) {
    const code = char.codePointAt(0) ?? 0
    const step = previous < 0 ? 0 : code - previous
    const predictable = previous >= 0 && (step === 0 || (Math.abs(step) === 1 && step === previousStep))
    total += predictable ? 0.2 : 1
    previous = code
    previousStep = step
  }
  return total
}

/** Rough strength of a new password, or null while nothing is typed. */
export function assessPassword(password: string, identity: string[] = []): PasswordStrength | null {
  if (!password) return null
  if (password.length < 8) {
    return { score: 0, label: 'Too short', hint: 'Use at least 8 characters.' }
  }

  const lower = password.toLowerCase()
  const names = identity
    .map((name) => name.split('@')[0].toLowerCase())
    .filter((name) => name.length >= 4)
  if (names.some((name) => lower.includes(name))) {
    return { score: 0, label: 'Weak', hint: 'Do not include your email or username.' }
  }

  const bits = effectiveLength(password) * Math.log2(Math.max(poolSize(password), 2))
  let index = bits < 30 ? 0 : bits < 45 ? 1 : bits < 65 ? 2 : 3
  let hint =
    index === 3
      ? 'Looks good.'
      : 'Longer is stronger: try a phrase of several words, or add length and a mix of characters.'

  if (COMMON_WORDS.some((word) => lower.includes(word)) && index > 1) {
    index = 1
    hint = 'Avoid common words and patterns.'
  }
  return { ...LEVELS[index], hint }
}
