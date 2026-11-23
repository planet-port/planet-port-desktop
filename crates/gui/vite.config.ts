import nodePath from 'node:path';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': nodePath.resolve(__dirname, 'src') },
  },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
});
