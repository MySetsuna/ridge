import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

export function renameArtifacts({ rootDir = root, fsImpl = fs, io = console } = {}) {
  const version = JSON.parse(fsImpl.readFileSync(path.join(rootDir, 'package.json'), 'utf-8')).version;
  const bundleDir = path.join(rootDir, 'target', 'release', 'bundle');
  const outputDir = path.join(rootDir, 'release');
  if (!fsImpl.existsSync(outputDir)) fsImpl.mkdirSync(outputDir);
  let copied = 0;
  for (const folder of ['nsis', 'msi']) {
    const folderPath = path.join(bundleDir, folder);
    if (!fsImpl.existsSync(folderPath)) continue;
    for (const file of fsImpl.readdirSync(folderPath)) {
      const ext = folder === 'nsis' ? '.exe' : '.msi';
      if (!file.includes(`_${version}_`) || !file.endsWith(ext)) continue;
      const sourcePath = path.join(folderPath, file);
      const destPath = path.join(outputDir, `ridge_${version}_x64-setup${ext}`);
      fsImpl.copyFileSync(sourcePath, destPath);
      io.log(`Copied and renamed ${sourcePath} to ${destPath}`);
      copied++;
    }
  }
  return copied;
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  renameArtifacts();
}
