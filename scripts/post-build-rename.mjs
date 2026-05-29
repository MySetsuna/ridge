import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf-8'));
const version = pkg.version;
const productName = 'ridge';
const arch = 'x64';

const bundleDir = path.join(root, 'src-tauri', 'target', 'release', 'bundle');
const outputDir = path.join(root, 'release');

if (!fs.existsSync(outputDir)) {
  fs.mkdirSync(outputDir);
}

// Check nsis and msi subdirectories if they exist
const formats = ['nsis', 'msi'];

formats.forEach(folder => {
  const folderPath = path.join(bundleDir, folder);
  if (fs.existsSync(folderPath)) {
    const files = fs.readdirSync(folderPath);
    files.forEach(file => {
      if (file.endsWith(`.${folder === 'nsis' ? 'exe' : 'msi'}`)) {
        const sourcePath = path.join(folderPath, file);
        const newName = `${productName}_${version}_${arch}-setup${path.extname(file)}`;
        const destPath = path.join(outputDir, newName);
        fs.copyFileSync(sourcePath, destPath);
        console.log(`Copied and renamed ${sourcePath} to ${destPath}`);
      }
    });
  }
});
