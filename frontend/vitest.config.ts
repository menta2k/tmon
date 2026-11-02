import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  test: {
    environment: 'jsdom',
    include: ['tests/unit/**/*.spec.ts'],
    setupFiles: ['tests/unit/setup.ts'],
    coverage: {
      provider: 'v8',
      include: ['src/**/*.{ts,vue}'],
      // Chart internals (ECharts module registration, GeoJSON loading) need a real canvas.
      exclude: [
        'src/main.ts',
        'src/**/*.d.ts',
        'src/components/charts/echarts.ts',
        'src/components/charts/worldMap.ts',
      ],
      reporter: ['text', 'text-summary', 'html'],
      thresholds: { lines: 80, statements: 80, functions: 80, branches: 75 },
    },
  },
})
