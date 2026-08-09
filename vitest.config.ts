import { defineConfig } from 'vitest/config';
import { resolve } from 'path';

export default defineConfig({
  resolve: {
    alias: {
      '$lib': resolve(__dirname, 'src/lib'),
      '@ridge/remote': resolve(__dirname, 'packages/remote/src'),
    },
  },
  test: {
    include: [
      'src/**/*.{test,spec}.{ts,js}',
      'packages/remote/**/*.{test,spec}.{ts,js}',
      'scripts/**/*.{test,spec}.{ts,js,mjs}',
    ],
    environment: 'node',
    globals: true,
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'clover', 'json', 'lcov'],
      include: [
        'src/**/*.{ts,js}',
        'packages/remote/src/**/*.ts',
        'scripts/**/*.mjs',
      ],
      exclude: ['**/*.test.*', '**/*.spec.*', '**/*.d.ts', '**/node_modules/**'],
      // Sonar consumes this report for the project baseline. Keep the source
      // set broad; targeted coverage must not masquerade as project coverage.
      thresholds: {
        lines: 10,
        functions: 10,
        branches: 5,
        statements: 10,
      },
    },
  },
});
