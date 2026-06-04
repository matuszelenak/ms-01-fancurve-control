import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: {
      // During `npm run dev`, forward API calls to the Rust server.
      '/api': 'http://localhost:8090',
    },
  },
})
