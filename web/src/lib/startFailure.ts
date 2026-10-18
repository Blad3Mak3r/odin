import type { NavigateFunction } from 'react-router-dom'
import { toast } from 'sonner'

/** Reports a failed start or restart, with a shortcut to the recorded startup details. */
export function toastStartFailure(error: Error, instanceId: string, navigate: NavigateFunction) {
  toast.error(error.message, {
    action: { label: 'Startup details', onClick: () => navigate(`/instance/${instanceId}/startup`) },
  })
}
