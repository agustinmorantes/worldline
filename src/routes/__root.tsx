import { createRootRoute, Outlet } from '@tanstack/react-router'
import { TanStackRouterDevtools } from '@tanstack/react-router-devtools'
import {ReactElement} from "react";
import {ThemeProvider} from "@/components/theme-provider.tsx";
import {Menubar, MenubarContent, MenubarItem, MenubarMenu, MenubarSeparator, MenubarShortcut, MenubarTrigger} from '@/components/ui/menubar';

const RootLayout = (): ReactElement => (
  <>
    <ThemeProvider defaultTheme="dark" storageKey="ui-theme">
      <div className="p-2 border-b-1 flex flex-row justify-between">
        <Menubar>
          <MenubarMenu>
            <MenubarTrigger>File</MenubarTrigger>
            <MenubarContent>
              <MenubarItem>
                New Tab <MenubarShortcut>⌘T</MenubarShortcut>
              </MenubarItem>
              <MenubarItem>New Window</MenubarItem>
              <MenubarSeparator />
              <MenubarItem>Share</MenubarItem>
              <MenubarSeparator />
              <MenubarItem>Print</MenubarItem>
            </MenubarContent>
          </MenubarMenu>
        </Menubar>
      </div>
      <div className="grow overflow-auto flex flex-col items-stretch">
        <Outlet />
      </div>
    </ThemeProvider>
    { import.meta.env.VITE_DEBUG !== undefined &&
      <TanStackRouterDevtools  />
    }
  </>
)

export const Route = createRootRoute({ component: RootLayout })