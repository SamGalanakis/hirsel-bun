import { defineConfig, type Plugin } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import solid from 'vite-plugin-solid';

/**
 * Strip data-test attributes from HTML in production builds.
 * These attributes are used for e2e testing and should not be in production.
 */
function stripDataTestAttrs(): Plugin {
  return {
    name: 'strip-data-test',
    apply: 'build',
    transformIndexHtml(html) {
      // Remove data-test="..." and :data-test="..." attributes
      return html.replace(/\s+:?data-test="[^"]*"/g, '');
    },
  };
}

export default defineConfig({
  plugins: [solid(), tailwindcss(), stripDataTestAttrs()],
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
