import base from '../vitest.config.ts';
import { mergeConfig } from 'vitest/config';
const config = mergeConfig(base, {
  plugins: [{
    name: 'native-runtime-build-mode', enforce: 'pre',
    transform(code, id) {
      const path = id.replaceAll('\\', '/').split('?')[0];
      if (path.endsWith('/src/lib/stores/paneTree.ts') || path.endsWith('/src/lib/stores/paneTree.runtime.test.ts')) {
        return code.replaceAll('import.meta.env.RIDGE_NATIVE_RUNTIME', 'true');
      }
    },
  }],
});
// Vitest SSR turns custom env values into strings; reproduce production's boolean define.
config.test.include = ['src/lib/stores/paneTree.runtime.test.ts'];
export default config;
