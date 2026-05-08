import { cloudflare } from "@cloudflare/vite-plugin";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig(({ command, mode }) => {
  const enableCloudflare =
    command === "serve" &&
    mode !== "test" &&
    process.env.CLOUDFLARE_VITE_DEV === "1";

  return {
    plugins: [react(), ...(enableCloudflare ? [cloudflare()] : [])],
    build: {
      outDir: "dist/client",
      emptyOutDir: true
    },
    server: {
      port: 5173
    }
  };
});
