import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  worker: { format: 'es' },
  server: {
    // Development only. Normal use serves the built app on the Rust node origin.
    proxy: { '/v1': 'http://127.0.0.1:4242' },
  },
  build: { target: 'es2022' },
});
