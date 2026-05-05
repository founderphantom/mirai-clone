import { cloudflare } from "@cloudflare/vite-plugin";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig(({ mode }) => ({
  plugins: [react(), ...(mode === "test" ? [] : [cloudflare()])],
  build: {
    outDir: "dist/client",
    emptyOutDir: true
  },
  server: {
    port: 5173
  }
}));
