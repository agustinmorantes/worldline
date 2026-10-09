import { ReactElement, ReactNode, useEffect, useLayoutEffect } from 'react'
import { MotionConfig } from 'motion/react'
import { QueryClientProvider } from '@tanstack/react-query'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { error as logError } from '@tauri-apps/plugin-log'
import { queryClient } from '@/app/query-client'
import { setTrayLabels } from '@/ipc/commands'
import { useLocale, useT } from '@/shared/i18n'
import { reducedMotionConfig } from '@/shared/motion/motion'
import { usePrefs } from '@/shared/prefs/prefs'
import { TooltipProvider } from '@/shared/ui/Tooltip'

/** Any CSS color (`--bg` is oklch) as sRGB bytes, by painting one canvas pixel. */
function cssColorToRgb(color: string): [number, number, number] {
  const context = document.createElement('canvas').getContext('2d')
  if (context === null) return [10, 12, 17]
  context.fillStyle = color
  context.fillRect(0, 0, 1, 1)
  const [r, g, b] = context.getImageData(0, 0, 1, 1).data
  return [r, g, b]
}

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
  // The native window shows through while it resizes; paint it the tone's background, not white.
  useEffect(() => {
    getCurrentWebviewWindow().setBackgroundColor(cssColorToRgb(getComputedStyle(document.body).backgroundColor)).catch((err: unknown) => {
      void logError(`Couldn't set the window background: ${String(err)}`).catch(() => undefined)
    })
  }, [tone])
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
        <TooltipProvider>{children}</TooltipProvider>
      </MotionConfig>
    </QueryClientProvider>
  )
}
