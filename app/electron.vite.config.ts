import { defineConfig } from 'electron-vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

// The React-refresh preamble is an inline script, so the dev server needs 'unsafe-inline' in script-src.
// Production builds keep the strict policy from index.html.
const devCsp = (): import('vite').Plugin => ({
  name: 'aurora-dev-csp',
  transformIndexHtml: {
    order: 'pre',
    handler: (html, ctx) => (ctx.server ? html.replace("script-src 'self'", "script-src 'self' 'unsafe-inline'") : html),
  },
})

const alias = { '@shared': resolve('src/shared'), '@protocol': resolve('../shared/protocol') }

export default defineConfig({
  main: { resolve: { alias }, build: { outDir: 'out/main' } },
  preload: { resolve: { alias }, build: { outDir: 'out/preload' } },
  renderer: {
    root: 'src/renderer',
    resolve: { alias },
    plugins: [react(), devCsp()],
    build: { outDir: 'out/renderer', rollupOptions: { input: resolve('src/renderer/index.html') } },
  },
})
