import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
// @ts-expect-error type error without @types/node package
import process from "node:process";
import pkg from "./package.json";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react()],

  // 版本号只有一个来源：package.json。界面里显示它，避免手动同步漏改。
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },

  // 多页面入口：主窗口 index.html + 划词弹窗 popup.html + 截图覆盖层 overlay.html
  // + 截图识别结果面板 ocr.html + 轻提示 notice.html
  build: {
    rollupOptions: {
      input: {
        main: "index.html",
        popup: "popup.html",
        overlay: "overlay.html",
        ocr: "ocr.html",
        notice: "notice.html",
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
