/**
 * Whether the window must refuse to navigate to `url`. The only allowed target is the app's own
 * page `appUrl`: the same file for a packaged build, or the same origin for the dev server. A file
 * dropped outside a drop zone would otherwise replace the app with `file:///…/track.flac`.
 */
export function shouldBlockNavigation(url: string, appUrl: string): boolean {
  let to: URL
  let app: URL
  try {
    to = new URL(url)
    app = new URL(appUrl)
  } catch {
    return true
  }
  if (to.protocol !== app.protocol) return true
  if (to.protocol === 'file:') return to.pathname !== app.pathname
  if (to.protocol === 'http:' || to.protocol === 'https:') return to.origin !== app.origin
  return true
}
