export default {
  app: {
    name: "Hirsel",
    identifier: "dev.hirsel.app",
    version: "0.1.0",
  },
  build: {
    bun: {
      entrypoint: "src/bun/index.ts",
    },
    views: {
      main: {
        entrypoint: "src/views/main/index.ts",
      },
    },
  },
  copy: {
    "src/views/main/index.html": "views/main/index.html",
    "src/views/main/styles.css": "views/main/styles.css",
  },
  macos: {
    codesign: false,
    notarize: false,
  },
  linux: {
    bundleCef: false,
  },
  windows: {
    bundleCef: false,
  },
};
