import { createRootRoute, Outlet, useLocation } from '@tanstack/react-router'
import { TanStackRouterDevtools } from '@tanstack/react-router-devtools'
import { ReactElement, useEffect } from "react"
import { ThemeProvider } from "@/components/theme-provider"
import { Toaster } from "sonner"
import { Sidebar } from "@/components/Sidebar"
import { TitleBar } from "@/components/TitleBar"
import { motion, useAnimationControls } from "framer-motion"

const RootLayout = (): ReactElement => {
  const location = useLocation()
  const panelControls = useAnimationControls()

  useEffect(() => {
    panelControls.set({ y: 5 })
    void panelControls.start({
      y: 0,
      transition: {
        duration: 0.36,
        ease: [0.2, 0.8, 0.2, 1],
      },
    })
  }, [location.pathname])

  return (
    <ThemeProvider defaultTheme="dark" storageKey="ui-theme">
      <div className="flex flex-col h-screen w-screen overflow-hidden bg-background text-foreground">
        <TitleBar />
        <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 flex flex-col relative overflow-hidden isolate z-10">
          <motion.div
            animate={panelControls}
            initial={false}
            className="flex-1 min-h-0 overflow-hidden"
          >
            <Outlet />
          </motion.div>
          <Toaster theme="dark" position="top-right" expand={false} richColors />
        </main>
        </div>
      </div>
      {import.meta.env.DEV && <TanStackRouterDevtools />}
    </ThemeProvider>
  )
}

export const Route = createRootRoute({ component: RootLayout })
