import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [tailwindcss()],
  root: 'src',
  build: {
    outDir: '../dist',
    emptyOutDir: true,
  },
  server: {
    port: 1420,
    strictPort: true,
    hmr: {
      overlay: false,
    },
  },
  optimizeDeps: {
    include: ['@tauri-apps/api'],
  },
});
