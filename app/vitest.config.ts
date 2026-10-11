import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { fileURLToPath } from 'node:url'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@shared': fileURLToPath(new URL('./src/shared', import.meta.url)),
      '@protocol': fileURLToPath(new URL('../shared/protocol', import.meta.url)),
    },
  },
  test: { include: ['test/**/*.test.{ts,tsx}'], environment: 'node', setupFiles: ['test/setup.ts'] },
})
