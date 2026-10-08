import { QueryClient, QueryCache, MutationCache } from '@tanstack/react-query'
import { getT, type Messages } from '@/shared/i18n'
import { reportError } from '@/shared/errors/report'
import { isFolderUnreachable } from '@/features/library/sync-activity'

/** Per query/mutation: the localized message shown when it fails. */
interface ErrorMeta extends Record<string, unknown> {
  errorMessage?: (t: Messages) => string
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
      // An unreachable cloud folder is shown in the sync card and settings, not as a toast.
      if (isFolderUnreachable(error)) return
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
