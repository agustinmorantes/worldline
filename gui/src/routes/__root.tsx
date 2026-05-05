import { createRootRoute, Outlet, useLocation } from '@tanstack/react-router'
import { TanStackRouterDevtools } from '@tanstack/react-router-devtools'
import { ReactElement } from "react"
import { ThemeProvider } from "@/components/theme-provider"
import { Toaster } from "sonner"
import { Sidebar } from "@/components/Sidebar"
import { TitleBar } from "@/components/TitleBar"
import { motion, AnimatePresence } from "framer-motion"

const RootLayout = (): ReactElement => {
  const location = useLocation()

  return (
    <ThemeProvider defaultTheme="dark" storageKey="ui-theme">
      <div className="flex flex-col h-screen w-screen overflow-hidden bg-background text-foreground">
        <TitleBar />
        <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 flex flex-col relative overflow-hidden isolate z-10">
          <AnimatePresence mode="wait">
            <motion.div
              key={location.pathname}
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -4 }}
              transition={{ duration: 0.15, ease: "easeOut" }}
              className="flex-1 overflow-auto"
            >
              <Outlet />
            </motion.div>
          </AnimatePresence>
          <Toaster theme="dark" position="top-right" expand={false} richColors />
        </main>
        </div>
      </div>
      {import.meta.env.DEV && <TanStackRouterDevtools />}
    </ThemeProvider>
  )
}

export const Route = createRootRoute({ component: RootLayout })
