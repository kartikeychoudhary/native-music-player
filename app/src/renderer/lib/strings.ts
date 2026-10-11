/** All UI copy added in M2 (spec §1: one module, so i18n can come later). */
export const S = Object.freeze({
  app: 'aurora',
  nav: { home: 'Home', search: 'Search', albums: 'Albums', artists: 'Artists', genres: 'Genres', playlists: 'Playlists', queue: 'Queue', signal: 'Signal path', settings: 'Settings' },
  expandSidebar: 'Expand sidebar',
  collapseSidebar: 'Collapse sidebar',
  unknownArtist: 'Unknown artist',
  unknownAlbum: 'Unknown album',
  notFound: 'This view does not exist.',
  goHome: 'Go home',
  cancel: 'Cancel',
  save: 'Save',
  dismiss: 'Dismiss',
  missingFile: 'File missing',
  unplayable: (reason: string) => `Can't play: ${reason}`,
  scanning: (processed: number, total: number) => (total ? `Scanning ${processed} / ${total}` : 'Scanning…'),
})
