import { createRootRoute, Outlet } from '@tanstack/react-router'
import { ReactElement } from "react"
import { Sidebar } from "@/features/library/Sidebar"
import { TitleBar } from "@/app/shell/TitleBar"
import { Shell } from "@/app/shell/Shell"
import { Toasts } from "@/app/shell/Toasts"
import { TourHost } from "@/features/onboarding/TourHost"

export const RootLayout = (): ReactElement => (
  <TourHost>
    <Shell titleBar={<TitleBar />} sidebar={<Sidebar />}>
      <Outlet />
    </Shell>
    <Toasts />
  </TourHost>
)

export const Route = createRootRoute({ component: RootLayout })
