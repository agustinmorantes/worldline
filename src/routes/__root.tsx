import { createRootRoute, Link, Outlet } from '@tanstack/react-router'
import { TanStackRouterDevtools } from '@tanstack/react-router-devtools'
import {ReactElement} from "react";
import {ThemeProvider} from "@/components/theme-provider.tsx";

const RootLayout = (): ReactElement => (
  <>
    <ThemeProvider defaultTheme="dark" storageKey="ui-theme">
      <div className="p-2 flex gap-2">
        <Link to="/" className="[&.active]:font-bold">
          Home
        </Link>{' '}
        <Link to="/about" className="[&.active]:font-bold">
          About
        </Link>
      </div>
      <hr />
      <Outlet />
    </ThemeProvider>
    <TanStackRouterDevtools  />
  </>
)

export const Route = createRootRoute({ component: RootLayout })