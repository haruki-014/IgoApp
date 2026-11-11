import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// 開発時は /api と /ws をローカルの server へ中継する(本番では nginx が同じことをする)。
const backend = process.env.IGO_BACKEND ?? 'http://127.0.0.1:18080';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: backend, changeOrigin: true },
      '/ws': { target: backend, ws: true, changeOrigin: true },
    },
  },
  build: {
    target: 'es2022',
    sourcemap: false,
  },
});
