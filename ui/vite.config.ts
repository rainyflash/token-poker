import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { resolve } from "node:path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
  build: {
    target: "chrome124",
    sourcemap: true,
    cssCodeSplit: false,
    lib: {
      entry: resolve(import.meta.dirname, "src/main.tsx"),
      name: "TokenHoldemPlugin",
      formats: ["iife"],
      fileName: () => "token-holdem.js",
      cssFileName: "token-holdem",
    },
  },
});
