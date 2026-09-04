import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri 开发环境下 Vite 需要固定端口，且忽略 src-tauri 变更
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
