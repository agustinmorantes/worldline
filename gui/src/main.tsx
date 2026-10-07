import * as ReactDOM from 'react-dom/client'
import { RouterProvider, createRouter } from '@tanstack/react-router'

import "./app/index.css"

// Import the generated route tree
import { routeTree } from './app/routeTree.gen'
import { listenNotifications } from "@/app/events"
import { AppProviders } from "@/app/AppProviders"
import { queryClient } from '@/app/query-client'
import { loadPrefs } from '@/shared/prefs/prefs'

// Create a new router instance
const router = createRouter({
  routeTree,
  context: {
    queryClient,
  },
})

// Register the router instance for type safety
declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}

async function start(): Promise<void> {
  // `bun run dev:mock`: the UI on plain `vite dev`, with the backend replaced by fixtures.
  if (import.meta.env.VITE_IPC_MOCKS === 'true') {
    const { installMocks } = await import('@/ipc/mocks')
    installMocks()
  }

  await loadPrefs()
  listenNotifications()

  const rootElement = document.getElementById('root')
  if (!rootElement) throw new Error('Failed to find the root element')

  if (!rootElement.innerHTML) {
    const root = ReactDOM.createRoot(rootElement)
    root.render(
      <AppProviders>
        <RouterProvider router={router} />
      </AppProviders>,
    )
  }
}

void start()
