import { createHash } from 'node:crypto';
import path from 'node:path';
export function cdpApplicationIdentifier(directory) {
  const absolute = path.resolve(directory);
  const normalized = process.platform === 'win32' ? absolute.toLowerCase() : absolute;
  return `com.tauri-app.ridge.cdp.${createHash('sha256').update(normalized).digest('hex').slice(0, 12)}`;
}
