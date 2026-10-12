import { QueryClient, QueryCache, MutationCache } from '@tanstack/react-query'
import { getT, type Messages } from '@/shared/i18n'
import { reportError } from '@/shared/errors/report'
import { syncBlock } from '@/features/library/sync-activity'

/** Per query/mutation: the localized message shown when it fails. */
interface ErrorMeta extends Record<string, unknown> {
  errorMessage?: (t: Messages) => string
  /** Failures are not reported; the UI falls back on its own. */
  quiet?: boolean
}

declare module '@tanstack/react-query' {
  interface Register {
    queryMeta: ErrorMeta
    mutationMeta: ErrorMeta
  }
}

export const queryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (error, query) => {
      // A blocked sync is shown in the sync card and settings, not as a toast.
      if (syncBlock(error) !== null) return
      if (query.meta?.quiet === true) return
      const t = getT()
      reportError(query.meta?.errorMessage?.(t) ?? t.errors.loadFailed, error)
    },
  }),
  mutationCache: new MutationCache({
    onError: (error, _variables, _context, mutation) => {
      // A mutation with its own onError reports the failure itself.
      if (mutation.options.onError) return
      const t = getT()
      reportError(mutation.meta?.errorMessage?.(t) ?? t.errors.actionFailed, error)
    },
  }),
  defaultOptions: {
    queries: {
      staleTime: 1000 * 60 * 5, // 5 minutes
      retry: 1,
    },
  },
})
