import { defineConfig } from 'electron-vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'
import { bundledPackages } from './build-tools/bundledPackages.ts'

// The React-refresh preamble is an inline script, so the dev server needs 'unsafe-inline' in script-src.
// Production builds keep the strict policy from index.html.
const devCsp = (): import('vite').Plugin => ({
  name: 'aurora-dev-csp',
  transformIndexHtml: {
    order: 'pre',
    handler: (html, ctx) => (ctx.server ? html.replace("script-src 'self'", "script-src 'self' 'unsafe-inline'") : html),
  },
})

// Where bundledPackages writes its lists; cwd-relative like the aliases (npm scripts run from app/).
const generated = resolve('build/generated')
const alias = { '@shared': resolve('src/shared'), '@protocol': resolve('../shared/protocol') }

export default defineConfig({
  main: { resolve: { alias }, plugins: [bundledPackages('main', generated)], build: { outDir: 'out/main' } },
  preload: { resolve: { alias }, plugins: [bundledPackages('preload', generated)], build: { outDir: 'out/preload' } },
  renderer: {
    root: 'src/renderer',
    resolve: { alias },
    plugins: [react(), devCsp(), bundledPackages('renderer', generated)],
    build: { outDir: 'out/renderer', rollupOptions: { input: resolve('src/renderer/index.html') } },
  },
})
