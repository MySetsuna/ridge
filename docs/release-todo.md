# Release Todo —— 只有你这边能做的事

> 代码侧已全部完成（发布工作流、MIT 许可证、vendored-openssl Intel 交叉编译、三平台签名接线）。
> 下面是需要**你的账号 / 机器 / 付费 / 决策**才能推进的事项。配置细节见 [release-signing.md](./release-signing.md)。
> v0.0.9 已发布为 Latest（8 产物全平台齐，含 macOS Intel）。

## 🟦 A. Windows 免费签名 — SignPath（已让我接好 workflow）
> 前置已满足：MIT 许可证已在 `main`、GitHub 已识别。下面全在 SignPath 官网 + 仓库 Settings，**我无法替你登录/审批**。
> **A 的第 1 步审核耗时，建议先提交申请。**

- [ ] 1. 注册 + 申请开源计划：https://signpath.io 注册 → 提交 `MySetsuna/ridge` 到 **Foundation / Open Source** 计划（人工审核，可能几天）。
- [ ] 2. 控制台建三样（审核通过后）：
  - [ ] 一个 **Project**（记下 project slug）
  - [ ] 一个 **Signing Policy** —— 先建 `test-signing`（验证管线、秒签不耗额度），再建 `release-signing`
  - [ ] 一个 **Artifact Configuration**（识别 zip 内 `.exe`/`.msi` 并各自 Authenticode 签名）
- [ ] 3. 取凭据：记下 **Organization ID**（GUID）+ 建一个 **CI User** 并生成 **API Token**。
- [ ] 4. 写进仓库（secret 值只有你能填；变量可让我代跑）：
  ```bash
  gh secret   set SIGNPATH_API_TOKEN          # CI User 的 API token
  gh secret   set SIGNPATH_ORGANIZATION_ID    # 组织 GUID
  gh variable set SIGNPATH_PROJECT_SLUG --body "你的-project-slug"
  gh variable set SIGNPATH_POLICY_SLUG  --body "test-signing"   # 先 test，通了再改 release-signing
  gh variable set SIGNPATH_ENABLED      --body "true"           # 总开关
  ```
- [ ] 5. 打个 tag 验证（如 `v0.0.10`）→ 看 Windows 的 `.exe/.msi` 是否被签名替换。

## 🍎 B. macOS 签名（可选，唯一干净路 = $99/年）
- [ ] 1. 买 Apple Developer Program（$99/年）。
- [ ] 2. 建 “Developer ID Application” 证书 → 导出 `.p12`（设密码）。
- [ ] 3. 生成 App 专用密码（appleid.apple.com）+ 记下 10 位 **Team ID**。
- [ ] 4. 配 6 个 secret：
  ```bash
  base64 -i DeveloperID.p12 | gh secret set APPLE_CERTIFICATE
  gh secret set APPLE_CERTIFICATE_PASSWORD
  gh secret set APPLE_SIGNING_IDENTITY      # "Developer ID Application: 名字 (TEAMID)"
  gh secret set APPLE_ID
  gh secret set APPLE_PASSWORD              # App 专用密码
  gh secret set APPLE_TEAM_ID
  ```
- [ ] 5. 告诉我「Apple 证书已就位」→ 我把 `APPLE_*` env 块加回 workflow（**不能提前留空加，会让构建挂**，已踩过）。

## 🐧 C. Linux 签名（可选，免费，价值偏低）
- [ ] 本地生成 GPG key（你持有私钥）→ `gh secret set GPG_PRIVATE_KEY`（有口令再加 `GPG_PASSPHRASE`）→ 把公钥贴 README。详见 release-signing.md。
- [ ] （更实用：后续上 Flathub / Snap，也只有你能注册。）

## 🧪 D. 真机验证（只有你有这些机器）
- [ ] Windows：装 `ridge_0.0.9_x64-setup.exe`，确认能装能跑。
- [ ] Linux：`sudo apt install ./ridge_0.0.9_amd64.deb`（或跑 AppImage）确认能跑。
- [ ] macOS **Apple Silicon**：装 `ridge_0.0.9_aarch64.dmg`，首次右键→打开。
- [ ] macOS **Intel**：装 `ridge_0.0.9_x64.dmg`（**新加的，最该验**），首次右键→打开。

## ⚙️ E. 决策 / 其它（按需）
- [ ] 确认 `LICENSE` 版权署名 “Jack Jiang and Ridge contributors” 是否就用这个；要改名/改机构告诉我。
- [ ] 仓库 **Actions 权限已被我设成读写**（否则 workflow 创建不了 Release）。有安全顾虑想收紧再说，但发布工作流需要它。
- [ ] 是否把 `develop` 的发布相关改动合进 `main`（`main` 现落后 develop 35 个提交）——你的分支策略，我不擅自合。

---

**优先级**：A（SignPath）最高 → D（Intel 真机验证）→ B/C 看预算和需要。
