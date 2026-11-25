import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

function parsePort(raw: string | undefined): number | undefined {
  if (!raw || !/^\d+$/.test(raw)) return undefined;
  const port = Number(raw);
  return Number.isInteger(port) && port > 0 && port <= 65535 ? port : undefined;
}

const port = parsePort(process.env.FACTORY_PORT);
const webPort = parsePort(process.env.FACTORY_WEB_PORT);

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    outDir: "dist",
  },
  server: {
    host: "127.0.0.1",
    ...(port !== undefined ? { port, strictPort: true } : {}),
    ...(webPort !== undefined
      ? {
          proxy: {
            "/ws": {
              target: `http://127.0.0.1:${webPort}`,
              ws: true,
            },
            "/images": `http://127.0.0.1:${webPort}`,
          },
        }
      : {}),
  },
});
