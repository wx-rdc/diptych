/// <reference types="vitest" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // v2.3 P0：pdf.worker 以 ESM inline 打包进主 bundle（blob worker），
  // 运行时零外部 fetch——根治公网 .mjs MIME/404/哈希错配三类故障
  worker: { format: 'es' },
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://127.0.0.1:39321',
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: './src/__tests__/setup.ts',
    include: ['src/**/*.test.{ts,tsx}'],
  },
});
