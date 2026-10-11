import { createHashHistory, createRootRoute, createRoute, createRouter, type RouterHistory } from '@tanstack/react-router'
import { Layout } from './Layout'
import { HomeView } from './views/HomeView'
import { NotFoundView } from './views/NotFoundView'
import { SignalView } from './views/SignalView'

const rootRoute = createRootRoute({ component: Layout, notFoundComponent: NotFoundView })
const homeRoute = createRoute({ getParentRoute: () => rootRoute, path: '/', component: HomeView })
const signalRoute = createRoute({ getParentRoute: () => rootRoute, path: '/signal', component: SignalView })

// Later tasks add their routes to this list.
export const routeTree = rootRoute.addChildren([homeRoute, signalRoute])

export function createAppRouter(history: RouterHistory = createHashHistory()) {
  return createRouter({ routeTree, history, defaultPreload: false })
}

declare module '@tanstack/react-router' {
  interface Register { router: ReturnType<typeof createAppRouter> }
}
