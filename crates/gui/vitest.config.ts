import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// 仅跑 src/test 下的逻辑层单测；产物构建走 package.json 的 build 脚本(tsc + vite build)，
// 与本文件无关(tsconfig 已把 src/test 排除出编译范围)
export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    // globals 开启后 @testing-library/react 自动注册 afterEach cleanup
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/test/**/*.test.{ts,tsx}'],
  },
});
