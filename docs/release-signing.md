# Release 代码签名指南（Windows / macOS / Linux）

> 现状：v0.0.9 的所有产物**均未签名**。本文说明每个平台「如何拿到签名」「有没有免费办法」，
> 以及如何接进 `.github/workflows/release.yml`。workflow 已为三平台留好挂载点——加上对应
> secret 即生效，不加则照旧产未签名包（零影响）。

## 一图速览

| 平台 | 不签名后果 | 免费且「干净」？ | 最低成本路径 | workflow 现状 |
|---|---|---|---|---|
| **Linux** | 基本无影响 | ✅ 完全免费 | 自管 GPG key | ✅ 已接线（缺 secret 自跳过） |
| **Windows** | SmartScreen 警告 | ✅ 开源免费（SignPath） | SignPath(开源免费) / Azure(~$10 月) / Certum(~€89 年) | ✅ 已接线（开 `SIGNPATH_ENABLED` 变量即启用） |
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
用户照样被拦）。本仓 license 前置 **已满足**：仓库为 public 且默认分支 `main` 带 MIT LICENSE
（GitHub 已识别为 MIT），满足 SignPath OSS 的开源前置。

### SignPath.io Foundation（开源免费，已接线）

workflow 里 `Stage / Upload / Sign with SignPath / Re-upload` 四步已就绪，仅在仓库变量
`SIGNPATH_ENABLED=true` 时启用（未配置=不签名、零影响）。流程：tauri 出未签名 `.exe`/`.msi`
→ `actions/upload-artifact` 上传 → `signpath/github-action-submit-signing-request` 云端签名
→ 下载签名件 → `gh release upload --clobber` 覆盖回 Release。

**一次性配置（都在 SignPath 控制台 + 本仓 Settings 做，我无法替你完成账号注册）：**
1. 去 https://signpath.io 注册，按 **Foundation / Open Source** 计划提交本仓（需 OSI 许可证，已满足）。
2. 审核通过后，在 SignPath 控制台建：一个 **Project**（记下 project slug）、一个 **Signing Policy**
   （如 `release-signing`，记下 slug）、一个 **Artifact Configuration**（让它识别 zip 内的 `.exe`/`.msi`
   并各自 Authenticode 签名）。拿到 **Organization ID**（GUID）和一个 **CI User API Token**。
3. 在本仓 **Settings → Secrets and variables → Actions** 配：
   ```bash
   gh secret set SIGNPATH_API_TOKEN          # CI User 的 API token
   gh secret set SIGNPATH_ORGANIZATION_ID    # 组织 GUID
   gh variable set SIGNPATH_PROJECT_SLUG --body "ridge"            # 你建的 project slug
   gh variable set SIGNPATH_POLICY_SLUG  --body "release-signing"  # 你建的 signing policy slug
   gh variable set SIGNPATH_ENABLED      --body "true"             # 总开关，开启签名
   ```
4. 下个 tag（如 `v0.0.10`）的 Windows `.exe`/`.msi` 即被 SignPath 签名后再发布。
   > 首次跑建议先用 `signing-policy = test-signing`（测试策略，秒签不耗审批额度）验证管线，
   > 通了再切 `release-signing`。SignPath 控制台可看每次签名请求的状态/日志。

### 其它（不想用 SignPath 时）

1. **Azure Trusted Signing（对 CI 最省事，~$9.99/月）**
   - 个人开发者也能用，需身份验证，无需自备 U 盾。有官方 `azure/trusted-signing-action`。
   - 流程：tauri build 出未签名 `.exe`/`.msi` → action 签 → 重新上传到 Release。

2. **Certum 开源代码签名（~€89/年）**：便宜，但**私钥必须在硬件 U 盾上**（CA/B 规则），CI 自动化不便。

3. **标准 OV / EV（$200–600/年）**：OV 靠下载量慢慢攒 SmartScreen 信誉；EV 即时信誉、需 U 盾。

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

1. **Windows**：MIT 许可证已就位、SignPath 四步已接线——只差去 SignPath 注册开源计划、建
   project/policy、配 2 secret + 3 变量（含 `SIGNPATH_ENABLED=true`），下个 tag 即免费签名。
2. **Linux**：随时可免费开（上面 4 条命令）。最实用的是后续上 Flathub。
3. **macOS**：预算到位就买 Apple $99/年；否则维持「右键打开」并在 README/Release 写明。

> workflow 挂载点：Windows = `Stage/Upload/Sign with SignPath/Re-upload` 四步（`SIGNPATH_ENABLED`
> 变量开关，已接线）；Linux = `GPG sign Linux artifacts` 步骤（无 secret 自跳过，已接线）；
> macOS = 有证书后按上面把 `APPLE_*` env 块加进 `Build & publish`（**勿留空**）。
