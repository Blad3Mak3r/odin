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

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  // A `FormData` body must NOT get a manual content-type: the browser sets
  // one itself (including the multipart boundary), and overriding it here
  // would break the upload.
  const isFormData = init?.body instanceof FormData
  const response = await fetch(`/api${path}`, {
    headers: init?.body && !isFormData ? { 'content-type': 'application/json' } : undefined,
    ...init,
  })

  if (!response.ok) {
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
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'POST', body: body !== undefined ? JSON.stringify(body) : undefined }),
  put: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'PUT', body: body !== undefined ? JSON.stringify(body) : undefined }),
  delete: <T>(path: string) => request<T>(path, { method: 'DELETE' }),
  upload: <T>(path: string, formData: FormData) =>
    request<T>(path, { method: 'POST', body: formData }),
}
