import { defineConfig } from 'electron-vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

const alias = { '@shared': resolve('src/shared'), '@protocol': resolve('../shared/protocol') }

export default defineConfig({
  main: { resolve: { alias }, build: { outDir: 'out/main' } },
  preload: { resolve: { alias }, build: { outDir: 'out/preload' } },
  renderer: {
    root: 'src/renderer',
    resolve: { alias },
    plugins: [react()],
    build: { outDir: 'out/renderer', rollupOptions: { input: resolve('src/renderer/index.html') } },
  },
})
