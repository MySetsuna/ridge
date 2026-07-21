import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf-8'));
const version = pkg.version;
const productName = 'ridge';
const arch = 'x64';

// 工作区合并（S1 ridge-core 抽取）后，src-tauri 成为虚拟工作区成员，
// 产物目录从 src-tauri/target 移到工作区根 target/。bundle 现位于 <root>/target/release/bundle。
const bundleDir = path.join(root, 'target', 'release', 'bundle');
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
      // 只挑当前版本的安装包。bundle 目录从不清理，跨构建会积累旧版本产物
      // （ridge_0.0.8_x64-setup.exe 等）；若不按版本过滤，会把每个旧 exe 都复制到
      // 同一个 `ridge_<version>_x64-setup.exe` 目标名、后者覆盖前者 → release/ 里
      // 的「当前版本」文件最终装的是某个旧版本的内容（曾把 0.0.13 写成 0.0.8）。
      if (!file.includes(`_${version}_`)) return;
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
