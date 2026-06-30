import tailwindcss from "@tailwindcss/vite";
import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vite";

const DEFAULT_BACKEND_ORIGIN = "http://127.0.0.1:18080";

// 读取后端联调地址，默认跟随 application.yaml 的本地端口。
export function backendOriginFromEnv(env = process.env) {
  return env.REMOTE_VOICE_BACKEND_ORIGIN?.trim() || DEFAULT_BACKEND_ORIGIN;
}

// 开发态使用根路径模拟 Rust 页面路由，构建态保留 /ui/ 资源前缀。
export function appBase(command) {
  return command === "build" ? "/ui/" : "/";
}

// 代理 API 和 WebSocket，避免 Vite dev server 把根路径后端请求当作前端资源。
export function backendProxy(target) {
  return {
    "/api": {
      target,
      changeOrigin: true,
    },
    "/ws": {
      target,
      changeOrigin: true,
      ws: true,
    },
  };
}

export default defineConfig(({ command }) => ({
  root: new URL(".", import.meta.url).pathname,
  base: appBase(command),
  plugins: [vue(), tailwindcss()],
  server: {
    proxy: backendProxy(backendOriginFromEnv()),
  },
  build: {
    outDir: "../static/dist",
    emptyOutDir: true,
    rollupOptions: {
      output: {
        entryFileNames: "assets/app.js",
        chunkFileNames: "assets/[name].js",
        assetFileNames: (assetInfo) => {
          if (assetInfo.name?.endsWith(".css")) {
            return "assets/index.css";
          }
          return "assets/[name][extname]";
        },
      },
    },
  },
}));
