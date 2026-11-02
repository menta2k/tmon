import { useMessage } from 'naive-ui'
import { errorMessage } from '@/utils/errors'

export interface Notify {
  success: (text: string) => void
  error: (err: unknown, context?: string) => void
  /** Runs `action`, toasting `successText` or a friendly error. Returns true on success. */
  run: (action: () => Promise<unknown>, successText?: string, context?: string) => Promise<boolean>
}

/** Thin wrapper over Naive UI's message API with consistent error text. */
export function useNotify(): Notify {
  const message = useMessage()

  const notify: Notify = {
    success: (text) => {
      message.success(text)
    },
    error: (err, context) => {
      message.error(errorMessage(err, context), { duration: 6000 })
    },
    run: async (action, successText, context) => {
      try {
        await action()
        if (successText) notify.success(successText)
        return true
      } catch (err) {
        notify.error(err, context)
        return false
      }
    },
  }
  return notify
}
