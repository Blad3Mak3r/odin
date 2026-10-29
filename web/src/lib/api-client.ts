// All requests use relative paths so the same built bundle works whether
// it's served by Vite's dev proxy (see vite.config.ts) or embedded directly
// in the `odin serve` binary in production.

export class ApiError extends Error {
  status: number

  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

let csrfToken: string | null = null

/** Token the server expects in `X-CSRF-Token` on every non-GET request. */
export function setCsrfToken(token: string | null) {
  csrfToken = token
}

let readOnly = false

/** Viewers cannot change anything; the server refuses it, this just answers sooner. */
export function setReadOnly(value: boolean) {
  readOnly = value
}

let onUnauthorized: (() => void) | undefined

/** Called when a non-auth endpoint answers 401, i.e. the session has expired. */
export function setUnauthorizedHandler(handler: () => void) {
  onUnauthorized = handler
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  // A `FormData` body must NOT get a manual content-type: the browser sets
  // one itself (including the multipart boundary), and overriding it here
  // would break the upload.
  if (readOnly && init?.method && init.method !== 'GET' && !path.startsWith('/auth/')) {
    throw new ApiError(403, 'This account is read-only. Ask an administrator to make the change.')
  }
  const isFormData = init?.body instanceof FormData
  const headers: Record<string, string> = {}
  if (init?.body && !isFormData) headers['content-type'] = 'application/json'
  if (init?.method && init.method !== 'GET' && csrfToken) headers['x-csrf-token'] = csrfToken
  const response = await fetch(`/api${path}`, { ...init, headers })

  if (!response.ok) {
    if (response.status === 401 && !path.startsWith('/auth/')) onUnauthorized?.()
    const body = await response.json().catch(() => null)
    const message = body && typeof body.error === 'string' ? body.error : response.statusText
    throw new ApiError(response.status, message)
  }

  if (response.status === 204) {
    return undefined as T
  }
  return (await response.json()) as T
}

export const api = {
  get: <T>(path: string, init?: RequestInit) => request<T>(path, init),
  post: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'POST', body: body !== undefined ? JSON.stringify(body) : undefined }),
  put: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'PUT', body: body !== undefined ? JSON.stringify(body) : undefined }),
  delete: <T>(path: string) => request<T>(path, { method: 'DELETE' }),
  upload: <T>(path: string, formData: FormData) =>
    request<T>(path, { method: 'POST', body: formData }),
}
