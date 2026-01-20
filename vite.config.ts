import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import { htmlIncludes } from './vite-html-includes';

export default defineConfig({
  plugins: [htmlIncludes(), tailwindcss()],
  root: 'src',
  publicDir: '../public',
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
