const { defineConfig } = require("vite");
const path = require("node:path");

module.exports = defineConfig({
  define: { __APP_VERSION__: JSON.stringify(require("./package.json").version) },
  root: path.resolve(__dirname, "src/renderer"),
  server: {
    host: "127.0.0.1",
    port: 5183,
    strictPort: true
  },
  preview: {
    host: "127.0.0.1",
    port: 4173,
    strictPort: true
  },
  build: {
    outDir: path.resolve(__dirname, "dist/renderer"),
    emptyOutDir: true
  }
});
