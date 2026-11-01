import { fileURLToPath, URL } from 'node:url'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'

// `pnpm dev` talks to a running ref-web (`ref-web --no-open --port 7480`); open
// http://127.0.0.1:5173/#key=K once (K: the key ref-web prints), the page keeps it for this origin.
// Use 127.0.0.1 throughout, not localhost. The channel is encrypted end to end through the proxy.
const REF_WEB = process.env.REF_WEB_URL ?? 'http://127.0.0.1:7480' // another port: set REF_WEB_URL

export default defineConfig({
  publicDir: '../assets/app/web', // favicons
  plugins: [vue()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    proxy: {
      // `secure: false`: a ref-web with TLS has a self-signed certificate.
      '/api': { target: REF_WEB, changeOrigin: true, secure: false },
      '/ws': {
        target: REF_WEB,
        ws: true,
        changeOrigin: true,
        secure: false,
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
