import { StrictMode } from 'react'
import * as ReactDOM from 'react-dom/client'
import { RouterProvider, createRouter } from '@tanstack/react-router'
import { QueryClientProvider } from '@tanstack/react-query'
import { ReactQueryDevtools } from '@tanstack/react-query-devtools'

import "./index.css"

// Import the generated route tree
import { routeTree } from './routeTree.gen'
import {listenNotifications} from "@/lib/notification-listener.ts";

import { queryClient } from './lib/query-client'

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

listenNotifications();

// Render the app
const rootElement = document.getElementById('root')
if(!rootElement) throw new Error('Failed to find the root element')

if (!rootElement.innerHTML) {
  const root = ReactDOM.createRoot(rootElement)
  root.render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
        <ReactQueryDevtools initialIsOpen={false} />
      </QueryClientProvider>
    </StrictMode>,
  )
}

