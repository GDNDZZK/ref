import { fileURLToPath, URL } from 'node:url'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'

// `pnpm dev` talks to a running ref-web (`ref-web --no-open`); log in once at
// http://127.0.0.1:5173/login?token=T. Use 127.0.0.1 throughout, not localhost.
const REF_WEB = 'http://127.0.0.1:7480'

export default defineConfig({
  publicDir: '../assets/app/web', // favicons
  plugins: [vue()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    proxy: {
      '/api': { target: REF_WEB, changeOrigin: true },
      '/login': { target: REF_WEB, changeOrigin: true },
      '/ws': {
        target: REF_WEB,
        ws: true,
        changeOrigin: true,
        // ref-web checks Origin against its own address.
        configure: (proxy) => {
          proxy.on('proxyReqWs', (request) => request.setHeader('origin', REF_WEB))
        },
      },
    },
  },
  test: {
    environment: 'happy-dom',
    include: ['src/**/*.test.ts', 'scripts/**/*.test.ts'],
  },
})
