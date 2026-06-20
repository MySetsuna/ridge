# Release 代码签名指南（Windows / macOS / Linux）

> 现状：v0.0.9 的所有产物**均未签名**。本文说明每个平台「如何拿到签名」「有没有免费办法」，
> 以及如何接进 `.github/workflows/release.yml`。workflow 已为三平台留好挂载点——加上对应
> secret 即生效，不加则照旧产未签名包（零影响）。

## 一图速览

| 平台 | 不签名后果 | 免费且「干净」？ | 最低成本路径 | workflow 现状 |
|---|---|---|---|---|
| **Linux** | 基本无影响 | ✅ 完全免费 | 自管 GPG key | ✅ 已接线（缺 secret 自跳过） |
| **Windows** | SmartScreen 警告 | ⚠️ 仅开源可免费 | SignPath(开源免费) / Azure(~$10 月) / Certum(~€89 年) | 文档（见下，未接线） |
| **macOS** | Gatekeeper 拦截，需右键打开 | ❌ 无免费干净方案 | Apple Developer $99/年 | 文档：有证书后手动加 env+secret |

---

## Linux —— 免费，已接线

Linux 没有 OS 级签名门禁；signature 只是给愿意校验的用户多一层保障。workflow 里
`GPG sign Linux artifacts (optional)` 这步会在配置了密钥时给 `.deb`/`.AppImage` 产 detached `.asc`。

**启用步骤：**
```bash
# 1) 本地生成发布密钥（按提示填 Real name / Email，建议设口令）
gpg --full-generate-key            # 选 RSA 4096 或 ed25519

# 2) 拿到 key id（sec 行冒号格式第 5 段）
gpg --list-secret-keys --keyid-format=long

# 3) 导出私钥（armored）
gpg --armor --export-secret-keys <KEYID> > ridge-release-private.asc

# 4) 存为仓库 secret（私钥不要进 git、不要贴聊天）
gh secret set GPG_PRIVATE_KEY < ridge-release-private.asc
gh secret set GPG_PASSPHRASE   # 若设了口令；没设可省略
rm ridge-release-private.asc    # 用完即删本地副本
```
**用户校验：** 下载 `xxx.deb` 和 `xxx.deb.asc` 后 `gpg --import 公钥 && gpg --verify xxx.deb.asc xxx.deb`。
把**公钥**（`gpg --armor --export <KEYID>`）放进 README 或 Release 说明即可。

**想要真正「免警告 + 自动更新信任」**：发到 **Flathub**（flatpak）或 **Snap Store**，二者免费、
替你构建+签名，用户从 store 装天然受信。比 detached 签名实用，但需各自的打包配置（独立任务）。

---

## Windows —— 免费仅限开源

警告来自 SmartScreen / Defender，必须用**受信任 CA 签发的 Authenticode 证书**（自签证书无效，
用户照样被拦）。本仓现状：**public 但无 LICENSE** → 不满足各开源免费计划的「OSI 许可证」前置。

> **先决条件（解锁免费/优惠的关键一步）**：加一个 OSI 许可证（如 `MIT` 或 `Apache-2.0`）。
> 选哪个是你对项目授权方式的决定，定了我可以直接加 `LICENSE`。

**选项（从免费到付费）：**

1. **SignPath.io Foundation（开源免费，推荐）**
   - 要求：稳定的开源项目 + OSI 许可证。审核通过后给免费证书 + 云签名 + GitHub Action。
   - 接线：在 `Build & publish` 之后加一步用 `signpath/github-action-submit-signing-request` 提交
     `.exe`/`.msi` 去云端签名，签好再 `gh release upload --clobber`。

2. **Azure Trusted Signing（对 CI 最省事，~$9.99/月）**
   - 个人开发者也能用，需身份验证，无需自备 U 盾。有官方 `azure/trusted-signing-action`。
   - 流程：tauri build 出未签名 `.exe`/`.msi` → action 签 → 重新上传到 Release。

3. **Certum 开源代码签名（~€89/年）**：便宜，但**私钥必须在硬件 U 盾上**（CA/B 规则），CI 自动化不便。

4. **标准 OV / EV（$200–600/年）**：OV 靠下载量慢慢攒 SmartScreen 信誉；EV 即时信誉、需 U 盾。

**接线骨架（以 Azure 为例，待你开通后我补全）：**
```yaml
- name: Sign Windows artifacts (Azure Trusted Signing)
  if: matrix.label == 'windows-x64' && env.AZURE_TS_ENDPOINT != ''
  uses: azure/trusted-signing-action@v0
  with:
    endpoint: ${{ secrets.AZURE_TS_ENDPOINT }}
    trusted-signing-account-name: ${{ secrets.AZURE_TS_ACCOUNT }}
    certificate-profile-name: ${{ secrets.AZURE_TS_PROFILE }}
    files-folder: src-tauri/target/release/bundle
    files-folder-filter: exe,msi
# 之后 gh release upload <tag> 已签名的 exe/msi --clobber
```

---

## macOS —— 唯一干净路是 Apple Developer $99/年

- 让用户**双击直接打开**（无「已损坏 / 无法验证」拦截）必须 **Developer ID 签名 + 公证(notarize)**，
  二者都依赖 **Apple Developer Program（$99/年）**。**公证没有免费档**。
- **免费的 ad-hoc 自签**只能让二进制能跑，但下载来的带 quarantine 仍被 Gatekeeper 拦——
  用户还是要右键→打开 或 `xattr -dr com.apple.quarantine Ridge.app`。

**开通后启用（两步：加 secret + 加 env）：**

> ⚠️ **不要提前把 APPLE_* env 加进 workflow 留空**——tauri-action 见到 `APPLE_CERTIFICATE`
> 存在就会去 `security import`，空值会让 macOS 构建直接挂（已踩过）。**只有真有证书时**才加下面两样。

1) 加 secret：
```bash
# 在 Apple Developer 后台建 “Developer ID Application” 证书，导出 .p12，base64：
base64 -i DeveloperID.p12 | gh secret set APPLE_CERTIFICATE
gh secret set APPLE_CERTIFICATE_PASSWORD     # 导出 .p12 时设的密码
gh secret set APPLE_SIGNING_IDENTITY         # 形如 "Developer ID Application: Name (TEAMID)"
gh secret set APPLE_ID                        # Apple 账号邮箱
gh secret set APPLE_PASSWORD                  # App 专用密码（appleid.apple.com 生成）
gh secret set APPLE_TEAM_ID                   # 10 位 Team ID
```
2) 把这个 env 块加进 `.github/workflows/release.yml` 的 `Build & publish (Tauri)` 步骤 `env:` 下：
```yaml
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
```
加完后下个 tag 的 macOS 包就自动签名 + 公证，dmg/app 双击即开。两个 arch 各自签。

---

## 落地建议（按性价比）

1. **Linux**：随时可免费开（上面 4 条命令）。最实用的是后续上 Flathub。
2. **Windows**：先定一个 OSI 许可证 → 申请 **SignPath 开源免费**（最划算）；不想等审核就用
   **Azure Trusted Signing**（~$10/月、CI 最顺）。
3. **macOS**：预算到位就买 Apple $99/年；否则维持「右键打开」并在 README/Release 写明。

> workflow 挂载点：Linux = `GPG sign Linux artifacts` 步骤（无 secret 自跳过，已接线）；
> macOS = 有证书后按上面把 `APPLE_*` env 块加进 `Build & publish`（**勿留空**）；Windows =
> 选定 provider 后补一步签名 action。
