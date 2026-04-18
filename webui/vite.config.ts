import path from "node:path";
import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";
import tailwindcss from "@tailwindcss/vite";

const backendPort = Number(process.env.HIRSEL_PORT ?? "8484");

export default defineConfig({
  plugins: [tailwindcss(), solidPlugin()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  server: {
    port: 5199,
    proxy: {
      "/api": `http://127.0.0.1:${backendPort}`,
    },
  },
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    chunkSizeWarningLimit: 3000,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("node_modules/monaco-editor/esm/vs/editor/editor.api")) {
            return "monaco-editor-api";
          }
          if (id.includes("node_modules/monaco-editor/esm/vs/editor/edcore.main")) {
            return "monaco-editor-core";
          }
          if (id.includes("node_modules/monaco-editor/esm/vs/editor/editor.worker")) {
            return "monaco-editor-worker";
          }
          if (id.includes("node_modules/monaco-editor/esm/vs/language/json")) {
            return "monaco-json";
          }
          if (id.includes("node_modules/monaco-editor/esm/vs/basic-languages/")) {
            return "monaco-languages";
          }
          if (id.includes("node_modules/three/")) {
            return "vendor-three";
          }
          if (id.includes("node_modules/@xterm/")) {
            return "vendor-xterm";
          }
          if (id.includes("node_modules/cytoscape") || id.includes("node_modules/mermaid")) {
            return "vendor-diagrams";
          }
          if (id.includes("node_modules/katex")) {
            return "vendor-katex";
          }
        },
      },
    },
  },
});
