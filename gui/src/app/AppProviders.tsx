import { ReactElement, ReactNode, useLayoutEffect } from 'react'
import { MotionConfig } from 'motion/react'
import { QueryClientProvider } from '@tanstack/react-query'
import { queryClient } from '@/app/query-client'
import { useLocale } from '@/shared/i18n'
import { reducedMotionConfig } from '@/shared/motion/motion'
import { usePrefs } from '@/shared/prefs/prefs'

/** Applies the preferences that live on <html> (language, tone, motion). */
function DocumentPrefs(): null {
  const { tone, motion } = usePrefs()
  const locale = useLocale()
  // Before the first paint, so a saved tone never flashes the default.
  useLayoutEffect(() => {
    const root = document.documentElement
    root.lang = locale
    root.dataset.tone = tone
    root.dataset.motion = motion
  }, [locale, tone, motion])
  return null
}

export function AppProviders({ children }: { children: ReactNode }): ReactElement {
  const { motion } = usePrefs()
  return (
    <QueryClientProvider client={queryClient}>
      <MotionConfig reducedMotion={reducedMotionConfig(motion)}>
        <DocumentPrefs />
        {children}
      </MotionConfig>
    </QueryClientProvider>
  )
}
