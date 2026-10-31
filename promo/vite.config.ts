import { resolve } from 'node:path'
import { defineConfig } from 'vite'

// Scenes import the theme, easing curves, icons and screenshots from outside promo/.
const REPO_ROOT = resolve(import.meta.dirname, '..')

export default defineConfig({
  server: { host: '127.0.0.1', fs: { allow: [REPO_ROOT] } },
  build: { outDir: 'dist', emptyOutDir: true },
})
