import { toast } from 'sonner'

export function errorMessage(error: unknown, fallback = 'Something went wrong'): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message
  }
  if (typeof error === 'string' && error.trim()) {
    return error
  }
  return fallback
}

export function showErrorToast(error: unknown, fallback = 'Something went wrong'): string {
  const message = errorMessage(error, fallback)
  toast.error(message, {
    action: {
      label: 'Copy',
      onClick: () => {
        void navigator.clipboard.writeText(message)
      }
    }
  })
  return message
}
