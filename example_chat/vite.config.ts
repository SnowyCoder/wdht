import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import { fileURLToPath } from 'url';
import windiCSS from 'vite-plugin-windicss';

// https://vitejs.dev/config/
export default defineConfig({
  resolve: {
    alias: {
      '@': fileURLToPath(new URL("./src", import.meta.url))
    }
  },
  plugins: [
    vue(),
    windiCSS(),
  ],
})
