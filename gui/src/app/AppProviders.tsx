import { ReactElement, ReactNode, useEffect, useLayoutEffect } from 'react'
import { MotionConfig } from 'motion/react'
import { QueryClientProvider } from '@tanstack/react-query'
import { error as logError } from '@tauri-apps/plugin-log'
import { queryClient } from '@/app/query-client'
import { setTrayLabels } from '@/ipc/commands'
import { useLocale, useT } from '@/shared/i18n'
import { reducedMotionConfig } from '@/shared/motion/motion'
import { usePrefs } from '@/shared/prefs/prefs'

/** Applies the preferences that live on <html> (language, tone, motion). */
function DocumentPrefs(): null {
  const { tone, motion } = usePrefs()
  const locale = useLocale()
  const t = useT()
  // Before the first paint, so a saved tone never flashes the default.
  useLayoutEffect(() => {
    const root = document.documentElement
    root.lang = locale
    root.dataset.tone = tone
    root.dataset.motion = motion
  }, [locale, tone, motion])
  // The tray speaks the GUI's language (ADR 0007).
  useEffect(() => {
    setTrayLabels({
      open: t.tray.open,
      exit: t.tray.exit,
      tooltip: t.tray.tooltip,
      hidden_hint_title: null,
      hidden_hint_body: t.tray.stillRunning,
    }).catch((err: unknown) => {
      void logError(`Couldn't set the tray labels: ${String(err)}`).catch(() => undefined)
    })
  }, [t])
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
