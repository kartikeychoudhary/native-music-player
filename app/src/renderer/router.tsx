import { createHashHistory, createRootRoute, createRoute, createRouter, type RouterHistory } from '@tanstack/react-router'
import { Layout } from './Layout'
import { AlbumsView } from './views/AlbumsView'
import { AlbumView } from './views/AlbumView'
import { ArtistsView } from './views/ArtistsView'
import { ArtistView } from './views/ArtistView'
import { GenresView } from './views/GenresView'
import { GenreView } from './views/GenreView'
import { HomeView } from './views/HomeView'
import { NotFoundView } from './views/NotFoundView'
import { PlaylistsView } from './views/PlaylistsView'
import { PlaylistView } from './views/PlaylistView'
import { QueueView } from './views/QueueView'
import { SearchView } from './views/SearchView'
import { SignalView } from './views/SignalView'

const rootRoute = createRootRoute({ component: Layout, notFoundComponent: NotFoundView })
const homeRoute = createRoute({ getParentRoute: () => rootRoute, path: '/', component: HomeView })
const albumsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/albums', component: AlbumsView })
const albumRoute = createRoute({ getParentRoute: () => rootRoute, path: '/albums/$albumId', component: AlbumView })
const artistsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/artists', component: ArtistsView })
const artistRoute = createRoute({ getParentRoute: () => rootRoute, path: '/artists/$artistId', component: ArtistView })
const genresRoute = createRoute({ getParentRoute: () => rootRoute, path: '/genres', component: GenresView })
const genreRoute = createRoute({ getParentRoute: () => rootRoute, path: '/genres/$genreId', component: GenreView })
const signalRoute = createRoute({ getParentRoute: () => rootRoute, path: '/signal', component: SignalView })
const searchRoute = createRoute({ getParentRoute: () => rootRoute, path: '/search', component: SearchView })
const playlistsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/playlists', component: PlaylistsView })
const playlistRoute = createRoute({ getParentRoute: () => rootRoute, path: '/playlists/$playlistId', component: PlaylistView })
const queueRoute = createRoute({ getParentRoute: () => rootRoute, path: '/queue', component: QueueView })

// Later tasks add their routes to this list.
export const routeTree = rootRoute.addChildren([homeRoute, albumsRoute, albumRoute, artistsRoute, artistRoute, genresRoute, genreRoute, searchRoute, playlistsRoute, playlistRoute, queueRoute, signalRoute])

export function createAppRouter(history: RouterHistory = createHashHistory()) {
  return createRouter({ routeTree, history, defaultPreload: false })
}

declare module '@tanstack/react-router' {
  interface Register { router: ReturnType<typeof createAppRouter> }
}
