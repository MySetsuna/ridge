import { defineConfig } from 'vitest/config';
import { resolve } from 'path';

const stripNodeShebangForModuleRunner = () => ({
  name: 'strip-node-shebang-for-module-runner',
  enforce: 'pre' as const,
  transform(code: string, id: string) {
    if (!id.split('?')[0].endsWith('.mjs') || !code.startsWith('#!')) return undefined;
    const newline = code.indexOf('\n');
    return { code: newline < 0 ? '' : code.slice(newline + 1), map: null };
  },
});

export default defineConfig({
  plugins: [stripNodeShebangForModuleRunner()],
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
