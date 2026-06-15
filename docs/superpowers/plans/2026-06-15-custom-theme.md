# 自定义主题（内置主题编辑器）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在设置面板「外观」页新增「自定义主题」入口卡，点击弹出大编辑弹窗（命名 / 终端背景图 + 透明度 / 全部配置项 / 实时预览），保存为带 `custom-` 前缀的用户主题入 `user-themes.json`，与内置主题合并展示，支持二次编辑 / 删除。

**Architecture:** 后端（ridge-core）新增用户主题持久化 + 目录合并 + 存图命令；前端扩展 themes/settings store 与一个 `activeBgImage` 信号；终端背景图复用已有 `PreMultiplied` 透明合成，纯 CSS 分层 + themeBridge 把 `term-bg` 喂成 alpha=0 实现，**不动 Rust 渲染热路径**；新增 `CustomThemeModal.svelte` 编辑弹窗（右侧 scoped 实时预览）；SettingsPanel 接入创建卡与编辑/删除入口。

**Tech Stack:** Rust (ridge-core + src-tauri Tauri commands)、Svelte 5（runes）、TypeScript、Tailwind、vitest（TS 单测）、`cargo test -p ridge-core`（Rust 单测）。

**关键既有事实（无需重新探查）：**
- 主题目录 `ridge.theme` 经 `get_theme_data` 读取（只读，发布版在 Program Files）。
- 当前主题 id 存 `<LOCALAPPDATA>/ridge/active-theme.txt`。`app_data_dir()` = `<LOCALAPPDATA>/ridge`（`packages/ridge-core/src/commands/theme.rs:89`）。
- `applyTheme(id)`（`src/lib/stores/settings.ts:149`）把 `theme.colors[k]` 写成 `--rg-{k}` CSS 变量。
- 终端 surface 已 `CompositeAlphaMode::PreMultiplied`；默认 cell 背景透明；`clear()`（`packages/ridge-term/src/render/webgpu.rs:473`）满屏底块用 `theme.bg` 且保留 alpha；`themeBridge.ts` 已把 `--rg-term-bg` 规整成带 alpha 的 hex8 喂内核。**term-bg 喂 alpha=0 → 满屏块透明 → CSS 层透出。**
- `src/lib/utils/cssColor.ts` 已有 `hex8WithAlpha(input, alpha)`（line 85）、`hex8(input)`。
- Tauri 命令注册在 `src-tauri/src/lib.rs:578` 的 `generate_handler!`（theme 命令在 722–723 行）。
- asset protocol scope 已是 `["**"]`（`src-tauri/tauri.conf.json:19`）——`convertFileSrc` 无需改配置。
- `convertFileSrc` 从 `@tauri-apps/api/core` 导入（参考 `src/lib/components/MarkdownPreview.svelte:24`）。
- `CoreError::io(msg)` / `CoreError::internal(msg)`（`packages/ridge-core/src/error.rs:112/117`），`.to_command_string()` 映射回 `Result<T,String>`。
- ridge-core 已依赖 `uuid`、`serde_json`、`tracing`。
- settings.* i18n 键在 `src/lib/i18n/messages.ts`（zh ~169 行，en ~389 行）。
- 检查 / 测试：`pnpm check`（svelte-check）、`pnpm test`（vitest）、`cargo test -p ridge-core`。

---

## File Structure

| 文件 | 动作 | 职责 |
|---|---|---|
| `packages/ridge-core/src/commands/theme.rs` | Modify | ThemeEntry 加 `bg_image`/`bg_image_opacity`；user-themes.json 读写/合并/id 规整；存图；新增单测 |
| `src-tauri/src/commands/theme.rs` | Modify | 薄封装 `#[tauri::command]`：`save_user_theme`/`delete_user_theme`/`save_theme_bg_image`/`get_theme_assets_dir`；`get_theme_data` 合并用户主题 |
| `src-tauri/src/lib.rs` | Modify | 注册 4 个新命令到 `generate_handler!` |
| `src/lib/stores/themes.ts` | Modify | 扩展 `ThemeEntry`；`refreshThemes`/`saveCustomTheme`/`deleteCustomTheme`/`slugifyThemeId`/`activeBgImage` store/`setActiveBgImage` |
| `src/lib/stores/themes.slug.test.ts` | Create | `slugifyThemeId` vitest 单测 |
| `src/lib/stores/settings.ts` | Modify | `applyTheme` 触发 `setActiveBgImage`；删除当前主题回退默认 |
| `src/lib/terminal/themeBridge.ts` | Modify | 订阅 `activeBgImage`；有图时把 `background` 改 alpha=0、`cursorAccent` 用实底色 |
| `src/lib/components/RidgePane.svelte` | Modify | canvas 之下插 `.rg-pane-bgimg` 背景图层 |
| `src/lib/components/CustomThemeModal.svelte` | Create | 编辑弹窗：表单 + 右侧 scoped 实时预览 + 保存 |
| `src/lib/components/customTheme.ts` | Create | 纯函数：颜色键常量、scoped CSS 变量字符串构造、表单→ThemeEntry 组装 |
| `src/lib/components/customTheme.test.ts` | Create | `customTheme.ts` 纯函数 vitest 单测 |
| `src/lib/components/SettingsPanel.svelte` | Modify | 创建卡 + 自定义卡编辑/删除入口 + 挂 `CustomThemeModal` |
| `src/lib/i18n/messages.ts` | Modify | 新增 `customTheme.*` 与 `settings.customThemeCard` 等文案（zh+en） |

---

## Task 1: ridge-core 数据模型 + 用户主题持久化 + 存图

**Files:**
- Modify: `packages/ridge-core/src/commands/theme.rs`
- Test: 同文件 `#[cfg(test)] mod tests`

- [ ] **Step 1.1: ThemeEntry 增加 bgImage/bgImageOpacity 字段**

修改 `packages/ridge-core/src/commands/theme.rs` 的 `ThemeEntry`（约 line 56）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeEntry {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub theme_type: String,
    pub loader: LoaderConfig,
    pub colors: HashMap<String, String>,
    /// 背景图资源文件名（相对 `theme-assets/`），仅自定义主题用。
    #[serde(rename = "bgImage", default, skip_serializing_if = "Option::is_none")]
    pub bg_image: Option<String>,
    /// 背景图透明度 0..1，缺省视为 1。
    #[serde(rename = "bgImageOpacity", default, skip_serializing_if = "Option::is_none")]
    pub bg_image_opacity: Option<f32>,
}
```

同时为 `src-tauri` 的测试 `sample_entry()`（`src-tauri/src/commands/theme.rs:175`）补上两个字段，避免破坏既有构造：在 `colors: {...}` 之后加 `bg_image: None, bg_image_opacity: None,`。

- [ ] **Step 1.2: 加常量与路径辅助**

在 `ACTIVE_THEME_FILE` 常量（line 73）下方追加：

```rust
/// 可写的用户自定义主题目录文件名（`<app_data_dir>/user-themes.json`）。
pub const USER_THEMES_FILE: &str = "user-themes.json";
/// 自定义主题 id 强制前缀，便于与内置主题区分（前端据此判可编辑/删除）。
pub const CUSTOM_ID_PREFIX: &str = "custom-";

fn user_themes_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(USER_THEMES_FILE)
}

/// 背景图资源目录 `<app_data_dir>/theme-assets`。
pub fn theme_assets_dir() -> PathBuf {
    app_data_dir().join("theme-assets")
}
```

- [ ] **Step 1.3: 写失败测试 —— 读空用户主题降级为空**

在 `mod tests` 末尾追加：

```rust
#[test]
fn read_user_themes_missing_file_is_empty() {
    let tmp = std::env::temp_dir().join("ridge-test-user-empty");
    let _ = std::fs::remove_dir_all(&tmp);
    let v = read_user_themes(&tmp);
    assert!(v.is_empty());
}
```

- [ ] **Step 1.4: 运行确认失败**

Run: `cargo test -p ridge-core read_user_themes_missing_file_is_empty`
Expected: 编译失败 —— `read_user_themes` 未定义。

- [ ] **Step 1.5: 实现 read/write 用户主题**

在 `set_active_theme`（line 217）之前追加：

```rust
/// 读用户自定义主题列表。文件缺失/解析失败 → 空 Vec（绝不让启动失败）。
pub fn read_user_themes(app_data_dir: &Path) -> Vec<ThemeEntry> {
    let path = user_themes_path(app_data_dir);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_str::<ThemeFile>(&content) {
        Ok(tf) => tf.themes,
        Err(e) => {
            tracing::warn!(target: "ridge::theme", error = %e, "user-themes.json 解析失败，忽略");
            Vec::new()
        }
    }
}

fn write_user_themes(app_data_dir: &Path, themes: &[ThemeEntry]) -> CoreResult<()> {
    std::fs::create_dir_all(app_data_dir).map_err(|e| CoreError::io(format!("create dir: {e}")))?;
    let tf = ThemeFile { version: 1, themes: themes.to_vec() };
    let json = serde_json::to_string_pretty(&tf)
        .map_err(|e| CoreError::internal(format!("serialize user themes: {e}")))?;
    std::fs::write(user_themes_path(app_data_dir), json)
        .map_err(|e| CoreError::io(format!("write user-themes.json: {e}")))
}
```

- [ ] **Step 1.6: 运行确认通过**

Run: `cargo test -p ridge-core read_user_themes_missing_file_is_empty`
Expected: PASS。

- [ ] **Step 1.7: 写失败测试 —— id 规整 slug + 去重**

```rust
#[test]
fn slugify_makes_custom_prefixed_unique_id() {
    let existing = vec!["custom-my-theme".to_string()];
    assert_eq!(make_custom_id("My Theme!!", &existing), "custom-my-theme-2");
    assert_eq!(make_custom_id("全新主题", &[]), "custom-全新主题");
    assert_eq!(make_custom_id("   ", &[]), "custom-theme");
}
```

- [ ] **Step 1.8: 运行确认失败**

Run: `cargo test -p ridge-core slugify_makes_custom_prefixed_unique_id`
Expected: 编译失败 —— `make_custom_id` 未定义。

- [ ] **Step 1.9: 实现 make_custom_id**

在 `read_user_themes` 上方追加：

```rust
/// 由 label 生成稳定、唯一、带 `custom-` 前缀的主题 id。
/// 规则：转小写、非字母数字(含 CJK 之外的符号)折叠成单个 `-`、去首尾 `-`；
/// 空则用 `theme`；与 `existing` 撞车则追加 `-2`/`-3`…。CJK 字符保留。
fn make_custom_id(label: &str, existing: &[String]) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in label.trim().chars() {
        if ch.is_alphanumeric() {
            for c in ch.to_lowercase() {
                slug.push(c);
            }
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    let slug = if slug.is_empty() { "theme".to_string() } else { slug };
    let base = format!("{CUSTOM_ID_PREFIX}{slug}");
    if !existing.iter().any(|e| e == &base) {
        return base;
    }
    let mut n = 2u32;
    loop {
        let cand = format!("{base}-{n}");
        if !existing.iter().any(|e| e == &cand) {
            return cand;
        }
        n += 1;
    }
}
```

- [ ] **Step 1.10: 运行确认通过**

Run: `cargo test -p ridge-core slugify_makes_custom_prefixed_unique_id`
Expected: PASS。

- [ ] **Step 1.11: 写失败测试 —— save/list/delete round-trip**

```rust
#[test]
fn save_and_delete_user_theme_round_trip() {
    let tmp = std::env::temp_dir().join("ridge-test-user-crud");
    let _ = std::fs::remove_dir_all(&tmp);
    let _guard = SetEnvGuard::new("LOCALAPPDATA", &tmp.to_string_lossy());
    // 新增（id 为空 → 自动规整）
    let mut entry = sample_user_entry("My Theme");
    entry.id = String::new();
    let saved = save_user_theme(entry).unwrap();
    assert_eq!(saved.id, "custom-my-theme");
    let listed = read_user_themes(&app_data_dir());
    assert_eq!(listed.len(), 1);
    // 编辑（同 id upsert）
    let mut edit = saved.clone();
    edit.label = "Renamed".into();
    let saved2 = save_user_theme(edit).unwrap();
    assert_eq!(saved2.id, "custom-my-theme"); // id 不变
    assert_eq!(read_user_themes(&app_data_dir())[0].label, "Renamed");
    // 删除
    delete_user_theme("custom-my-theme").unwrap();
    assert!(read_user_themes(&app_data_dir()).is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}
```

并在 tests 模块加一个构造辅助：

```rust
fn sample_user_entry(label: &str) -> ThemeEntry {
    ThemeEntry {
        id: format!("{CUSTOM_ID_PREFIX}tmp"),
        label: label.into(),
        theme_type: "dark".into(),
        loader: LoaderConfig {
            primary: "#fff".into(), secondary: "#000".into(),
            bg: None, accent_glow: None, stroke_width: None, corner_radius: None,
            draw_duration_ms: None, breathe_duration_ms: None, cross_delay_ms: None,
            fade_out_duration_ms: None, fill_opacity_primary: None, fill_opacity_secondary: None,
        },
        colors: HashMap::new(),
        bg_image: None,
        bg_image_opacity: None,
    }
}
```

- [ ] **Step 1.12: 运行确认失败**

Run: `cargo test -p ridge-core save_and_delete_user_theme_round_trip`
Expected: 编译失败 —— `save_user_theme`/`delete_user_theme` 未定义。

- [ ] **Step 1.13: 实现 save_user_theme / delete_user_theme**

在 `make_custom_id` 下方追加：

```rust
/// upsert 一个自定义主题到 user-themes.json，返回最终落盘的 entry。
/// id 为空或不带 `custom-` 前缀 → 视为新增、由 label 生成唯一 id；
/// 否则按 id 覆盖（编辑）。
pub fn save_user_theme(mut entry: ThemeEntry) -> CoreResult<ThemeEntry> {
    let dir = app_data_dir();
    let mut themes = read_user_themes(&dir);
    let is_new = entry.id.is_empty() || !entry.id.starts_with(CUSTOM_ID_PREFIX);
    if is_new {
        let existing: Vec<String> = themes.iter().map(|t| t.id.clone()).collect();
        entry.id = make_custom_id(&entry.label, &existing);
        themes.push(entry.clone());
    } else {
        match themes.iter_mut().find(|t| t.id == entry.id) {
            Some(slot) => *slot = entry.clone(),
            None => themes.push(entry.clone()),
        }
    }
    write_user_themes(&dir, &themes)?;
    Ok(entry)
}

/// 删除一个自定义主题及其背景图（best-effort）。
pub fn delete_user_theme(id: &str) -> CoreResult<()> {
    let dir = app_data_dir();
    let mut themes = read_user_themes(&dir);
    if let Some(pos) = themes.iter().position(|t| t.id == id) {
        if let Some(img) = themes[pos].bg_image.clone() {
            let _ = std::fs::remove_file(theme_assets_dir().join(img));
        }
        themes.remove(pos);
        write_user_themes(&dir, &themes)?;
    }
    Ok(())
}
```

- [ ] **Step 1.14: 运行确认通过**

Run: `cargo test -p ridge-core save_and_delete_user_theme_round_trip`
Expected: PASS。

- [ ] **Step 1.15: 写失败测试 —— get_theme_data 合并用户主题在内置之后**

```rust
#[test]
fn merge_appends_user_themes_after_base() {
    let base = ThemeFile {
        version: 1,
        themes: vec![sample_user_entry("Builtin")],
    };
    // 第一个伪装成内置（去掉 custom- 前缀）
    let mut base = base;
    base.themes[0].id = "endless-dark".into();
    let user = vec![sample_user_entry("U1")];
    let merged = merge_user_theme_list(base, user);
    assert_eq!(merged.themes.len(), 2);
    assert_eq!(merged.themes[0].id, "endless-dark");
    assert!(merged.themes[1].id.starts_with("custom-"));
}
```

- [ ] **Step 1.16: 运行确认失败**

Run: `cargo test -p ridge-core merge_appends_user_themes_after_base`
Expected: 编译失败 —— `merge_user_theme_list` 未定义。

- [ ] **Step 1.17: 实现合并辅助并接入 get_theme_data**

在 `get_theme_data`（line 159）上方追加纯函数：

```rust
/// 把用户主题追加到内置目录之后；id 撞车时丢弃用户那条（内置优先），
/// 实际不会撞车（用户 id 强制 custom- 前缀）。纯函数，便于单测。
pub fn merge_user_theme_list(mut base: ThemeFile, user: Vec<ThemeEntry>) -> ThemeFile {
    let have: std::collections::HashSet<String> =
        base.themes.iter().map(|t| t.id.clone()).collect();
    for u in user {
        if !have.contains(&u.id) {
            base.themes.push(u);
        }
    }
    base
}
```

并把 `get_theme_data()`（handle-free 版）`return tf;`（约 line 165）改为先合并：

```rust
                    if tf.version >= 1 && !tf.themes.is_empty() {
                        return merge_user_theme_list(tf, read_user_themes(&app_data_dir()));
                    }
```

- [ ] **Step 1.18: 运行确认通过**

Run: `cargo test -p ridge-core merge_appends_user_themes_after_base`
Expected: PASS。

- [ ] **Step 1.19: 写失败测试 —— 存图校验扩展名与体积**

```rust
#[test]
fn save_bg_image_validates_and_writes() {
    let tmp = std::env::temp_dir().join("ridge-test-bgimg");
    let _ = std::fs::remove_dir_all(&tmp);
    let _guard = SetEnvGuard::new("LOCALAPPDATA", &tmp.to_string_lossy());
    // 非法扩展名被拒
    assert!(save_theme_bg_image(vec![1, 2, 3], "exe").is_err());
    // 超限被拒
    assert!(save_theme_bg_image(vec![0u8; 21 * 1024 * 1024], "png").is_err());
    // 正常写入，返回文件名以 .png 结尾，文件确实存在
    let name = save_theme_bg_image(vec![0u8; 16], "PNG").unwrap();
    assert!(name.ends_with(".png"));
    assert!(theme_assets_dir().join(&name).exists());
    let _ = std::fs::remove_dir_all(&tmp);
}
```

- [ ] **Step 1.20: 运行确认失败**

Run: `cargo test -p ridge-core save_bg_image_validates_and_writes`
Expected: 编译失败 —— `save_theme_bg_image` 未定义。

- [ ] **Step 1.21: 实现 save_theme_bg_image + get_theme_assets_dir**

在 `delete_user_theme` 下方追加：

```rust
const ALLOWED_IMG_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];
const MAX_IMG_BYTES: usize = 20 * 1024 * 1024;

/// 把背景图字节写入 `theme-assets/<uuid>.<ext>`，返回文件名。
pub fn save_theme_bg_image(bytes: Vec<u8>, ext: &str) -> CoreResult<String> {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if !ALLOWED_IMG_EXT.contains(&ext.as_str()) {
        return Err(CoreError::internal(format!("unsupported image type: {ext}")));
    }
    if bytes.is_empty() || bytes.len() > MAX_IMG_BYTES {
        return Err(CoreError::internal(format!("image size out of range: {} bytes", bytes.len())));
    }
    let dir = theme_assets_dir();
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::io(format!("create theme-assets: {e}")))?;
    let name = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    std::fs::write(dir.join(&name), &bytes).map_err(|e| CoreError::io(format!("write image: {e}")))?;
    Ok(name)
}

/// 返回 theme-assets 目录绝对路径字符串（前端拼路径 + convertFileSrc 用）。
pub fn get_theme_assets_dir() -> String {
    theme_assets_dir().to_string_lossy().to_string()
}
```

- [ ] **Step 1.22: 运行确认通过 + 全量 core 测试**

Run: `cargo test -p ridge-core`
Expected: 全部 PASS（含既有用例）。

- [ ] **Step 1.23: Commit**

```bash
git add packages/ridge-core/src/commands/theme.rs src-tauri/src/commands/theme.rs
git commit -m "feat(theme): ridge-core 用户主题持久化 + 目录合并 + 存图命令

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: src-tauri 命令薄封装 + 注册

**Files:**
- Modify: `src-tauri/src/commands/theme.rs`
- Modify: `src-tauri/src/lib.rs:578` 区段（`generate_handler!`）

- [ ] **Step 2.1: 加 4 个 #[tauri::command] 薄封装**

在 `src-tauri/src/commands/theme.rs` 的 `set_active_theme`（line 114）下方追加：

```rust
/// 保存（新增/编辑）一个自定义主题，返回最终落盘的 entry。
#[tauri::command]
pub fn save_user_theme(entry: ThemeEntry) -> Result<ThemeEntry, String> {
    ridge_core::commands::theme::save_user_theme(entry).map_err(|e| e.to_command_string())
}

/// 删除一个自定义主题及其背景图。
#[tauri::command]
pub fn delete_user_theme(id: String) -> Result<(), String> {
    ridge_core::commands::theme::delete_user_theme(&id).map_err(|e| e.to_command_string())
}

/// 写入背景图字节，返回文件名（相对 theme-assets/）。
#[tauri::command]
pub fn save_theme_bg_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    ridge_core::commands::theme::save_theme_bg_image(bytes, &ext).map_err(|e| e.to_command_string())
}

/// 返回 theme-assets 目录绝对路径（前端 convertFileSrc 用）。
#[tauri::command]
pub fn get_theme_assets_dir() -> String {
    ridge_core::commands::theme::get_theme_assets_dir()
}
```

- [ ] **Step 2.2: get_theme_data（AppHandle 版）合并用户主题**

修改 `src-tauri/src/commands/theme.rs` 的 `get_theme_data`（line 130）：把 `if tf.version >= 1 && !tf.themes.is_empty() { return tf; }`（约 line 135-137）改为：

```rust
                    if tf.version >= 1 && !tf.themes.is_empty() {
                        let user = ridge_core::commands::theme::read_user_themes(
                            &ridge_core::commands::theme::app_data_dir(),
                        );
                        return ridge_core::commands::theme::merge_user_theme_list(tf, user);
                    }
```

- [ ] **Step 2.3: 注册命令**

在 `src-tauri/src/lib.rs` 的 `generate_handler!`（line 722-723 附近）`theme::set_active_theme,` 之后加：

```rust
            theme::save_user_theme,
            theme::delete_user_theme,
            theme::save_theme_bg_image,
            theme::get_theme_assets_dir,
```

- [ ] **Step 2.4: 编译验证**

Run: `cargo check -p ridge`
Expected: 通过（无 warning 级阻断；新命令被 `generate_handler!` 引用，不报 dead_code）。

> 注意（项目约定）：不要并行跑 `cargo check` 与常驻的 `tauri dev`。若 dev 常驻，等其重建即可，或单独跑一次 `cargo check -p ridge`。

- [ ] **Step 2.5: Commit**

```bash
git add src-tauri/src/commands/theme.rs src-tauri/src/lib.rs
git commit -m "feat(theme): src-tauri 自定义主题命令封装与注册 + get_theme_data 合并

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: 前端 store 扩展 + activeBgImage 信号

**Files:**
- Modify: `src/lib/stores/themes.ts`
- Create: `src/lib/stores/themes.slug.test.ts`
- Modify: `src/lib/stores/settings.ts`

- [ ] **Step 3.1: 写失败测试 —— slugifyThemeId**

Create `src/lib/stores/themes.slug.test.ts`：

```ts
import { describe, it, expect } from 'vitest';
import { slugifyThemeId } from './themes';

describe('slugifyThemeId', () => {
  it('lowercases and dashes non-alnum, adds custom- prefix', () => {
    expect(slugifyThemeId('My Theme!!')).toBe('custom-my-theme');
  });
  it('keeps CJK', () => {
    expect(slugifyThemeId('全新主题')).toBe('custom-全新主题');
  });
  it('falls back to theme on empty', () => {
    expect(slugifyThemeId('   ')).toBe('custom-theme');
  });
});
```

- [ ] **Step 3.2: 运行确认失败**

Run: `pnpm test -- themes.slug`
Expected: FAIL —— `slugifyThemeId` 未导出。

- [ ] **Step 3.3: 扩展 ThemeEntry + 实现 store CRUD 与 slug**

修改 `src/lib/stores/themes.ts`：

(a) `ThemeEntry`（line 27）加字段：

```ts
export interface ThemeEntry {
  id: string;
  label: string;
  type: 'dark' | 'light';
  loader: LoaderConfig;
  colors: Record<string, string>;
  bgImage?: string;        // theme-assets/ 下的文件名
  bgImageOpacity?: number; // 0..1，缺省视为 1
}
```

(b) 顶部 import 改为：

```ts
import { writable, get } from 'svelte/store';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
```

(c) 文件末尾追加（`initThemeSystem` 之后）：

```ts
/** 自定义主题 id 前缀（与 ridge-core CUSTOM_ID_PREFIX 对齐）。 */
export const CUSTOM_ID_PREFIX = 'custom-';

/** 是否自定义主题（可编辑/删除）。 */
export function isCustomTheme(id: string): boolean {
  return id.startsWith(CUSTOM_ID_PREFIX);
}

/** 由 label 生成 `custom-` 前缀 id（与后端规则一致，前端仅用于预测；最终以后端返回为准）。 */
export function slugifyThemeId(label: string): string {
  let slug = '';
  let prevDash = false;
  for (const ch of label.trim()) {
    if (/\p{L}|\p{N}/u.test(ch)) {
      slug += ch.toLowerCase();
      prevDash = false;
    } else if (!prevDash) {
      slug += '-';
      prevDash = true;
    }
  }
  slug = slug.replace(/^-+|-+$/g, '');
  return CUSTOM_ID_PREFIX + (slug || 'theme');
}

/** 重新从后端拉取合并后的主题目录（含用户主题）。 */
export async function refreshThemes(): Promise<void> {
  try {
    const tf = await invoke<ThemeFile>('get_theme_data');
    store.set(tf);
  } catch (e) {
    console.warn('refreshThemes failed', e);
  }
}

/** 保存（新增/编辑）自定义主题，返回后端规整后的 entry，并刷新 store。 */
export async function saveCustomTheme(entry: ThemeEntry): Promise<ThemeEntry> {
  const saved = await invoke<ThemeEntry>('save_user_theme', { entry });
  await refreshThemes();
  return saved;
}

/** 删除自定义主题并刷新 store。 */
export async function deleteCustomTheme(id: string): Promise<void> {
  await invoke('delete_user_theme', { id });
  await refreshThemes();
}

/** 把图片字节存到 theme-assets/，返回文件名。 */
export async function saveThemeBgImage(bytes: Uint8Array, ext: string): Promise<string> {
  return invoke<string>('save_theme_bg_image', { bytes: Array.from(bytes), ext });
}

// ── 活动主题背景图信号 ──────────────────────────────────────────────
export interface ActiveBgImage {
  url: string | null;     // convertFileSrc 后的可加载 URL
  opacity: number;        // 0..1
}
const bgImageStore = writable<ActiveBgImage>({ url: null, opacity: 1 });
export const activeBgImage = { subscribe: bgImageStore.subscribe };

let _assetsDir: string | null = null;
async function assetsDir(): Promise<string | null> {
  if (_assetsDir !== null) return _assetsDir;
  try {
    _assetsDir = await invoke<string>('get_theme_assets_dir');
  } catch {
    _assetsDir = null;
  }
  return _assetsDir;
}

/** 解析某主题的背景图为可加载 URL，更新 activeBgImage 信号。fire-and-forget。 */
export async function setActiveBgImage(themeId: string): Promise<void> {
  const t = getTheme(themeId);
  if (!t || !t.bgImage) {
    bgImageStore.set({ url: null, opacity: 1 });
    return;
  }
  const dir = await assetsDir();
  const sep = dir && dir.includes('\\') ? '\\' : '/';
  const url = dir ? convertFileSrc(`${dir}${sep}${t.bgImage}`) : null;
  bgImageStore.set({ url, opacity: t.bgImageOpacity ?? 1 });
}
```

- [ ] **Step 3.4: 运行确认通过**

Run: `pnpm test -- themes.slug`
Expected: PASS。

- [ ] **Step 3.5: settings.ts applyTheme 触发背景图信号**

修改 `src/lib/stores/settings.ts`：

(a) 顶部 import：

```ts
import { getTheme, setActiveBgImage } from './themes';
```

(b) `applyTheme`（line 149）函数体末尾（`for` 循环之后）追加：

```ts
  // 解析该主题的终端背景图（自定义主题专属，异步、fire-and-forget）。
  void setActiveBgImage(themeId);
```

- [ ] **Step 3.6: 类型检查**

Run: `pnpm check`
Expected: 无新增类型错误。

- [ ] **Step 3.7: Commit**

```bash
git add src/lib/stores/themes.ts src/lib/stores/themes.slug.test.ts src/lib/stores/settings.ts
git commit -m "feat(theme): themes/settings store 扩展 + 背景图信号

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: 终端背景图渲染（themeBridge alpha + RidgePane 图层）

**Files:**
- Modify: `src/lib/terminal/themeBridge.ts`
- Modify: `src/lib/components/RidgePane.svelte`

- [ ] **Step 4.1: themeBridge —— 有背景图时清 term-bg alpha**

修改 `src/lib/terminal/themeBridge.ts`：

(a) 顶部 import 追加：

```ts
import { hex8, hex8WithAlpha } from '$lib/utils/cssColor';
import { activeBgImage } from '$lib/stores/themes';
import { get } from 'svelte/store';
```

（注意：原有 `import { hex8 } from '$lib/utils/cssColor';` 合并为上面这行，避免重复导入。）

(b) `readRidgeTheme()` 内，把 `if (bg) out.background = bg;`（line 77）替换为：

```ts
	// 背景图激活时：把喂给内核的 term-bg alpha 清零，使 clear() 满屏底块透明，
	// canvas 背后的 CSS 层（纯色底 + 背景图）透出。cursorAccent 仍用实底色，
	// 否则光标块上的字形会变透明。
	const bgImageActive = get(activeBgImage).url !== null;
	if (bg) {
		out.background = bgImageActive ? (hex8WithAlpha(bg, 0) ?? bg) : bg;
	}
```

(c) 同函数内 `if (bg) out.cursorAccent = bg;`（line 84）替换为：

```ts
			if (bg) out.cursorAccent = bgImageActive ? (hex8(bg) ?? bg) : bg;
```

(d) `setupTerminalThemeBridge()` 内，在 `unsubscribeFont` 定义之后、`return () => {...}` 之前追加一个对 `activeBgImage` 的订阅，使背景图开关变化时重新推送：

```ts
	const unsubscribeBgImage = activeBgImage.subscribe(() => {
		push();
	});
```

并把末尾 `return () => { unsubscribeTheme(); unsubscribeFont(); _subscribed = false; };` 改为：

```ts
	return () => {
		unsubscribeTheme();
		unsubscribeFont();
		unsubscribeBgImage();
		_subscribed = false;
	};
```

- [ ] **Step 4.2: RidgePane —— canvas 之下插背景图层**

修改 `src/lib/components/RidgePane.svelte`：

(a) `<script>` 顶部 import 区追加（与现有 import 同风格）：

```ts
	import { activeBgImage } from '$lib/stores/themes';
```

(b) 容器元素（line 1715-1719，`class="rg-pane-container ..."`）内部、`{#if $settingsStore.terminalImeMode === 'ime'}` 那块 textarea **之前**，插入背景图层：

```svelte
	{#if $activeBgImage.url}
		<div
			class="rg-pane-bgimg"
			style="background-image: url('{$activeBgImage.url}'); opacity: {$activeBgImage.opacity};"
			aria-hidden="true"
		></div>
	{/if}
```

(c) 组件 `<style>` 区追加（若无 `<style>` 块则在文件末尾新建一个）：

```css
	.rg-pane-bgimg {
		position: absolute;
		inset: 0;
		background-size: cover;
		background-position: center;
		background-repeat: no-repeat;
		pointer-events: none;
		z-index: 0;
	}
```

> canvas 由 manager 动态插入为容器子节点，默认在文档流之上；`.rg-pane-bgimg` 用 `z-index:0` 且容器已 `position:relative`，canvas 无显式 z-index 时按后插入顺序覆于其上。若实测被遮挡，给背景层 `z-index:0`、canvas 不动即可（canvas 透明合成会透出本层）。

- [ ] **Step 4.3: 类型检查**

Run: `pnpm check`
Expected: 无新增类型错误。

- [ ] **Step 4.4: Commit**

```bash
git add src/lib/terminal/themeBridge.ts src/lib/components/RidgePane.svelte
git commit -m "feat(theme): 终端背景图渲染（RidgePane 图层 + themeBridge 清 term-bg alpha）

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: customTheme 纯函数 + CustomThemeModal 编辑弹窗

**Files:**
- Create: `src/lib/components/customTheme.ts`
- Create: `src/lib/components/customTheme.test.ts`
- Create: `src/lib/components/CustomThemeModal.svelte`

- [ ] **Step 5.1: 写失败测试 —— 颜色键常量 + scoped CSS 构造 + 表单组装**

Create `src/lib/components/customTheme.test.ts`：

```ts
import { describe, it, expect } from 'vitest';
import { CORE_COLOR_KEYS, ANSI_COLOR_KEYS, ALPHA_COLOR_KEYS, previewStyle, buildThemeEntry } from './customTheme';

describe('customTheme constants', () => {
  it('has 18 core keys incl. accent and term-bg', () => {
    expect(CORE_COLOR_KEYS).toHaveLength(18);
    expect(CORE_COLOR_KEYS).toContain('accent');
    expect(CORE_COLOR_KEYS).toContain('term-bg');
  });
  it('has 16 ansi keys', () => {
    expect(ANSI_COLOR_KEYS).toHaveLength(16);
    expect(ANSI_COLOR_KEYS).toContain('ansi-brightWhite');
  });
  it('marks rgba-style keys as alpha-bearing', () => {
    expect(ALPHA_COLOR_KEYS).toContain('glass');
    expect(ALPHA_COLOR_KEYS).not.toContain('bg');
  });
});

describe('previewStyle', () => {
  it('emits scoped --rg- vars from colors map', () => {
    const s = previewStyle({ bg: '#000', accent: '#fff' });
    expect(s).toContain('--rg-bg: #000;');
    expect(s).toContain('--rg-accent: #fff;');
  });
});

describe('buildThemeEntry', () => {
  it('assembles a custom ThemeEntry from form state', () => {
    const e = buildThemeEntry({
      id: '', label: 'My', type: 'dark',
      colors: { bg: '#000' }, loaderPrimary: '#aaa', loaderSecondary: '#bbb',
      bgImage: 'x.png', bgImageOpacity: 0.5,
    });
    expect(e.label).toBe('My');
    expect(e.colors.bg).toBe('#000');
    expect(e.loader.primary).toBe('#aaa');
    expect(e.bgImage).toBe('x.png');
    expect(e.bgImageOpacity).toBe(0.5);
  });
});
```

- [ ] **Step 5.2: 运行确认失败**

Run: `pnpm test -- customTheme`
Expected: FAIL —— `./customTheme` 模块不存在。

- [ ] **Step 5.3: 实现 customTheme.ts**

Create `src/lib/components/customTheme.ts`：

```ts
// 自定义主题编辑器的纯逻辑：颜色键清单、scoped 预览 CSS、表单→ThemeEntry 组装。
import type { ThemeEntry } from '$lib/stores/themes';

/** 常驻取色器的 18 个核心 UI 色（顺序即展示顺序）。 */
export const CORE_COLOR_KEYS = [
  'bg', 'bg-raised', 'surface', 'surface-2', 'glass',
  'border', 'border-bright', 'fg', 'fg-muted', 'accent',
  'accent-glow', 'term-bg', 'tui-bg', 'scrollbar', 'scrollbar-hover',
  'title-proc', 'title-sep', 'title-cwd',
] as const;

/** 进阶区的 16 个 ANSI 终端色。 */
export const ANSI_COLOR_KEYS = [
  'ansi-black', 'ansi-red', 'ansi-green', 'ansi-yellow',
  'ansi-blue', 'ansi-magenta', 'ansi-cyan', 'ansi-white',
  'ansi-brightBlack', 'ansi-brightRed', 'ansi-brightGreen', 'ansi-brightYellow',
  'ansi-brightBlue', 'ansi-brightMagenta', 'ansi-brightCyan', 'ansi-brightWhite',
] as const;

/** 这些键惯用 rgba（取色器旁显示 alpha 滑块）。 */
export const ALPHA_COLOR_KEYS = new Set<string>([
  'glass', 'border', 'border-bright', 'accent-glow', 'scrollbar', 'scrollbar-hover',
]);

/** 把 colors map 渲染成 scoped `--rg-*` 行内样式（仅作用于预览容器）。 */
export function previewStyle(colors: Record<string, string>): string {
  return Object.entries(colors)
    .map(([k, v]) => `--rg-${k}: ${v};`)
    .join(' ');
}

export interface ThemeFormState {
  id: string;
  label: string;
  type: 'dark' | 'light';
  colors: Record<string, string>;
  loaderPrimary: string;
  loaderSecondary: string;
  bgImage?: string;
  bgImageOpacity: number;
}

/** 表单 → 可保存的 ThemeEntry（id 留空交后端规整）。 */
export function buildThemeEntry(f: ThemeFormState): ThemeEntry {
  return {
    id: f.id,
    label: f.label.trim(),
    type: f.type,
    loader: { primary: f.loaderPrimary, secondary: f.loaderSecondary },
    colors: { ...f.colors },
    ...(f.bgImage ? { bgImage: f.bgImage } : {}),
    ...(f.bgImage ? { bgImageOpacity: f.bgImageOpacity } : {}),
  };
}
```

> 注意：`loader` 类型来自 `ThemeEntry.loader: LoaderConfig`，`LoaderConfig` 仅 `primary`/`secondary` 必填，其余可选——本期进阶 loader 数值旋钮先不接（YAGNI），只暴露 primary/secondary 颜色。

- [ ] **Step 5.4: 运行确认通过**

Run: `pnpm test -- customTheme`
Expected: PASS。

- [ ] **Step 5.5: 实现 CustomThemeModal.svelte**

Create `src/lib/components/CustomThemeModal.svelte`：

```svelte
<!-- src/lib/components/CustomThemeModal.svelte
     自定义主题编辑大弹窗。左列表单（命名/类型/基于/背景图+透明度/核心色/进阶），
     右列 scoped 实时预览。z-index 9996（高于 SettingsPanel 9994，低于 ContextMenu 9999）。
     仅在桌面（isTauri）可用：保存/存图/选图都依赖 Tauri 命令与对话框。 -->
<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { X } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { themeData, getTheme, saveCustomTheme, saveThemeBgImage, type ThemeEntry } from '$lib/stores/themes';
  import { setTheme } from '$lib/stores/settings';
  import {
    CORE_COLOR_KEYS, ANSI_COLOR_KEYS, ALPHA_COLOR_KEYS,
    previewStyle, buildThemeEntry, type ThemeFormState,
  } from './customTheme';
  import { hex8WithAlpha, hex8 } from '$lib/utils/cssColor';

  interface Props {
    open: boolean;
    editingId: string | null;   // null = 新建
    onClose: () => void;
  }
  let { open, editingId, onClose }: Props = $props();

  // ── 表单状态 ───────────────────────────────────────────────
  let form = $state<ThemeFormState>(blankForm());
  let baseId = $state<string>('endless-dark');
  let saving = $state(false);
  let errorMsg = $state<string | null>(null);
  let bgImageUrl = $state<string | null>(null); // 预览用 convertFileSrc URL

  function blankForm(): ThemeFormState {
    return {
      id: '', label: '', type: 'dark', colors: {},
      loaderPrimary: '#eeeeee', loaderSecondary: '#888888',
      bgImage: undefined, bgImageOpacity: 0.3,
    };
  }

  // 把一个现有主题的 colors/loader 灌进表单（“基于”起点 / 编辑预填）。
  function loadFrom(entry: ThemeEntry, keepIdLabel: boolean): void {
    form.colors = { ...entry.colors };
    form.type = entry.type;
    form.loaderPrimary = entry.loader.primary;
    form.loaderSecondary = entry.loader.secondary;
    if (keepIdLabel) {
      form.id = entry.id;
      form.label = entry.label;
      form.bgImage = entry.bgImage;
      form.bgImageOpacity = entry.bgImageOpacity ?? 0.3;
    }
  }

  // 打开时初始化：编辑→预填该主题；新建→基于 baseId 克隆。
  $effect(() => {
    if (!open) return;
    errorMsg = null;
    if (editingId) {
      const e = getTheme(editingId);
      if (e) { form = blankForm(); loadFrom(e, true); baseId = editingId; }
    } else {
      form = blankForm();
      const b = getTheme(baseId) ?? $themeData.themes[0];
      if (b) loadFrom(b, false);
    }
  });

  // baseId 切换（仅新建态）→ 重新克隆色板，保留已输入的 label。
  function onBaseChange(id: string): void {
    baseId = id;
    const b = getTheme(id);
    if (b) { const label = form.label; loadFrom(b, false); form.label = label; }
  }

  // 背景图预览 URL：随 form.bgImage 变化解析。
  $effect(() => {
    void resolveBgUrl(form.bgImage);
  });
  async function resolveBgUrl(name: string | undefined): Promise<void> {
    if (!name) { bgImageUrl = null; return; }
    try {
      const { convertFileSrc } = await import('@tauri-apps/api/core');
      const dir = await invoke<string>('get_theme_assets_dir');
      const sep = dir.includes('\\') ? '\\' : '/';
      bgImageUrl = convertFileSrc(`${dir}${sep}${name}`);
    } catch { bgImageUrl = null; }
  }

  async function pickImage(): Promise<void> {
    if (!isTauri()) return;
    const picked = await openDialog({
      multiple: false, directory: false,
      filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }],
    });
    if (typeof picked !== 'string') return;
    try {
      const { readFile } = await import('@tauri-apps/plugin-fs');
      const bytes = await readFile(picked);
      const ext = picked.split('.').pop() ?? 'png';
      form.bgImage = await saveThemeBgImage(bytes, ext);
    } catch (e) {
      errorMsg = String(e);
    }
  }

  function removeImage(): void { form.bgImage = undefined; }

  function setColor(key: string, value: string): void {
    form.colors = { ...form.colors, [key]: value };
  }
  // rgba 类色：用 hex(#rrggbb) + alpha(0..1) 合成 #rrggbbaa 存入。
  function setColorWithAlpha(key: string, hexPart: string, alpha: number): void {
    const v = hex8WithAlpha(hexPart, alpha) ?? hexPart;
    setColor(key, v);
  }
  // 取色器只认 #rrggbb：把任意色规整成 6 位 hex 喂给 <input type=color>。
  function hex6(v: string | undefined): string {
    const h = v ? hex8(v) : null;
    return h ? h.slice(0, 7) : '#000000';
  }
  function alphaOf(v: string | undefined): number {
    const h = v ? hex8(v) : null;
    if (!h || h.length < 9) return 1;
    return parseInt(h.slice(7, 9), 16) / 255;
  }

  const canSave = $derived(form.label.trim().length > 0 && !saving);

  async function save(): Promise<void> {
    if (!canSave) return;
    saving = true; errorMsg = null;
    try {
      const entry = buildThemeEntry({ ...form, id: editingId ?? '' });
      const saved = await saveCustomTheme(entry);
      setTheme(saved.id);
      onClose();
    } catch (e) {
      errorMsg = String(e);
    } finally {
      saving = false;
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') { e.stopPropagation(); onClose(); }
  }
</script>

<svelte:window onkeydown={open ? onKeydown : null} />

{#if open}
  <div
    class="fixed inset-0 bg-black/55 backdrop-blur-sm flex items-center justify-center"
    style="z-index: 9996;"
    role="presentation"
    onmousedown={(e) => { if (e.target === e.currentTarget) onClose(); }}
  >
    <div
      class="w-[940px] max-w-[94vw] h-[640px] max-h-[90vh] bg-[var(--rg-bg-raised)] border border-[var(--rg-border)] rounded-xl shadow-2xl flex flex-col overflow-hidden"
      role="dialog" aria-modal="true" aria-label={$t('customTheme.title')}
    >
      <header class="h-11 shrink-0 flex items-center justify-between px-4 border-b border-[var(--rg-border)]">
        <h2 class="text-[13px] font-medium text-[var(--rg-fg)]">
          {editingId ? $t('customTheme.editTitle') : $t('customTheme.newTitle')}
        </h2>
        <button type="button" class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)]" onclick={onClose} title={$t('settings.close')}>
          <X class="h-4 w-4" />
        </button>
      </header>

      <div class="flex-1 min-h-0 flex">
        <!-- 左：表单 -->
        <div class="w-[520px] shrink-0 overflow-y-auto rg-scroll p-4 space-y-4 border-r border-[var(--rg-border)]">
          <!-- 名称 -->
          <div>
            <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="ct-name">{$t('customTheme.name')}</label>
            <input id="ct-name" type="text" bind:value={form.label} placeholder={$t('customTheme.namePlaceholder')}
              class="w-full px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]" />
          </div>

          <!-- 类型 + 基于 -->
          <div class="flex gap-3">
            <div class="flex-1">
              <span class="block text-[12px] text-[var(--rg-fg)] mb-1">{$t('customTheme.type')}</span>
              <div class="inline-flex rounded-md border border-[var(--rg-border)] overflow-hidden">
                <button type="button" class="px-3 py-1 text-[12px] {form.type === 'dark' ? 'bg-[var(--rg-accent)] text-[var(--rg-bg)]' : 'text-[var(--rg-fg)]'}" onclick={() => form.type = 'dark'}>{$t('customTheme.dark')}</button>
                <button type="button" class="px-3 py-1 text-[12px] border-l border-[var(--rg-border)] {form.type === 'light' ? 'bg-[var(--rg-accent)] text-[var(--rg-bg)]' : 'text-[var(--rg-fg)]'}" onclick={() => form.type = 'light'}>{$t('customTheme.light')}</button>
              </div>
            </div>
            {#if !editingId}
              <div class="flex-1">
                <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="ct-base">{$t('customTheme.basedOn')}</label>
                <select id="ct-base" value={baseId} onchange={(e) => onBaseChange((e.currentTarget as HTMLSelectElement).value)}
                  class="w-full px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)]">
                  {#each $themeData.themes as th (th.id)}
                    <option value={th.id}>{th.label}</option>
                  {/each}
                </select>
              </div>
            {/if}
          </div>

          <!-- 背景图 -->
          <div>
            <span class="block text-[12px] text-[var(--rg-fg)] mb-1">{$t('customTheme.bgImage')}</span>
            <div class="flex items-center gap-2">
              <button type="button" class="px-2 py-1.5 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)] hover:bg-[var(--rg-surface-2)] text-[12px] text-[var(--rg-fg)]" onclick={pickImage}>{$t('customTheme.chooseImage')}</button>
              {#if form.bgImage}
                <button type="button" class="px-2 py-1.5 rounded border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]" onclick={removeImage}>{$t('customTheme.removeImage')}</button>
              {/if}
            </div>
            {#if form.bgImage}
              <div class="mt-2">
                <label class="block text-[11px] text-[var(--rg-fg-muted)] mb-1" for="ct-bgop">{$t('customTheme.bgOpacity')}: {Math.round(form.bgImageOpacity * 100)}%</label>
                <input id="ct-bgop" type="range" min="0" max="1" step="0.01" bind:value={form.bgImageOpacity} class="w-full accent-[var(--rg-accent)]" />
              </div>
            {/if}
          </div>

          <!-- 核心色 -->
          <div>
            <div class="text-[12px] text-[var(--rg-fg)] mb-2">{$t('customTheme.coreColors')}</div>
            <div class="grid grid-cols-2 gap-2">
              {#each CORE_COLOR_KEYS as key (key)}
                <div class="flex items-center gap-2">
                  <input type="color" value={hex6(form.colors[key])}
                    oninput={(e) => {
                      const hx = (e.currentTarget as HTMLInputElement).value;
                      if (ALPHA_COLOR_KEYS.has(key)) setColorWithAlpha(key, hx, alphaOf(form.colors[key]));
                      else setColor(key, hx);
                    }}
                    class="h-6 w-8 shrink-0 rounded border border-[var(--rg-border)] bg-transparent cursor-pointer" />
                  <span class="text-[11px] text-[var(--rg-fg-muted)] font-mono truncate flex-1">{key}</span>
                  {#if ALPHA_COLOR_KEYS.has(key)}
                    <input type="range" min="0" max="1" step="0.01" value={alphaOf(form.colors[key])}
                      oninput={(e) => setColorWithAlpha(key, hex6(form.colors[key]), Number((e.currentTarget as HTMLInputElement).value))}
                      class="w-12 accent-[var(--rg-accent)]" title="alpha" />
                  {/if}
                </div>
              {/each}
            </div>
          </div>

          <!-- 进阶：ANSI + loader -->
          <details class="rounded border border-[var(--rg-border)] p-2">
            <summary class="text-[12px] text-[var(--rg-fg)] cursor-pointer select-none">{$t('customTheme.advanced')}</summary>
            <div class="mt-2">
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-1">{$t('customTheme.loaderColors')}</div>
              <div class="flex gap-3 mb-3">
                <label class="flex items-center gap-2 text-[11px] text-[var(--rg-fg-muted)] font-mono">primary
                  <input type="color" bind:value={form.loaderPrimary} class="h-6 w-8 rounded border border-[var(--rg-border)] bg-transparent cursor-pointer" /></label>
                <label class="flex items-center gap-2 text-[11px] text-[var(--rg-fg-muted)] font-mono">secondary
                  <input type="color" bind:value={form.loaderSecondary} class="h-6 w-8 rounded border border-[var(--rg-border)] bg-transparent cursor-pointer" /></label>
              </div>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-1">{$t('customTheme.ansiColors')}</div>
              <div class="grid grid-cols-2 gap-2">
                {#each ANSI_COLOR_KEYS as key (key)}
                  <div class="flex items-center gap-2">
                    <input type="color" value={hex6(form.colors[key])} oninput={(e) => setColor(key, (e.currentTarget as HTMLInputElement).value)}
                      class="h-6 w-8 shrink-0 rounded border border-[var(--rg-border)] bg-transparent cursor-pointer" />
                    <span class="text-[11px] text-[var(--rg-fg-muted)] font-mono truncate">{key}</span>
                  </div>
                {/each}
              </div>
            </div>
          </details>
        </div>

        <!-- 右：实时预览 -->
        <div class="flex-1 min-w-0 p-4 flex flex-col">
          <div class="text-[12px] text-[var(--rg-fg-muted)] mb-2">{$t('customTheme.preview')}</div>
          <div class="flex-1 min-h-0 rounded-lg overflow-hidden border border-[var(--rg-border)] flex flex-col" style={previewStyle(form.colors)}>
            <!-- 假标题栏 -->
            <div class="h-8 flex items-center gap-2 px-3 shrink-0" style="background: var(--rg-glass);">
              <span class="text-[11px]" style="color: var(--rg-title-proc);">zsh</span>
              <span style="color: var(--rg-title-sep);">/</span>
              <span class="text-[11px]" style="color: var(--rg-title-cwd);">~/project</span>
              <span class="ml-auto text-[10px] px-1.5 py-0.5 rounded" style="background: color-mix(in srgb, var(--rg-accent) 20%, transparent); color: var(--rg-accent);">accent</span>
            </div>
            <div class="flex-1 min-h-0 flex">
              <!-- 假侧栏 -->
              <div class="w-10 shrink-0 flex flex-col items-center gap-2 py-2" style="background: var(--rg-surface);">
                <div class="h-4 w-4 rounded" style="background: var(--rg-accent);"></div>
                <div class="h-4 w-4 rounded" style="background: var(--rg-fg-muted);"></div>
              </div>
              <!-- 假终端区（铺背景图 + 文本） -->
              <div class="flex-1 min-w-0 relative" style="background: var(--rg-term-bg);">
                {#if bgImageUrl}
                  <div class="absolute inset-0" style="background-image: url('{bgImageUrl}'); background-size: cover; background-position: center; opacity: {form.bgImageOpacity};"></div>
                {/if}
                <div class="relative p-3 font-mono text-[11px] leading-5" style="color: var(--rg-fg);">
                  <div><span style="color: var(--rg-accent);">$</span> echo hello</div>
                  <div>hello</div>
                  <div>
                    <span style="color: {form.colors['ansi-green'] ?? '#28a745'};">ok</span>
                    <span style="color: {form.colors['ansi-red'] ?? '#e3342f'};">err</span>
                    <span style="color: {form.colors['ansi-blue'] ?? '#3366cc'};">info</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 底栏 -->
      <footer class="h-12 shrink-0 flex items-center justify-end gap-2 px-4 border-t border-[var(--rg-border)]">
        {#if errorMsg}<span class="mr-auto text-[11px] text-[var(--rg-ansi-red,#e3342f)] truncate">{errorMsg}</span>{/if}
        <button type="button" class="px-3 py-1.5 rounded border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]" onclick={onClose}>{$t('common.cancel')}</button>
        <button type="button" disabled={!canSave}
          class="px-3 py-1.5 rounded text-[12px] {canSave ? 'bg-[var(--rg-accent)] text-[var(--rg-bg)]' : 'bg-[var(--rg-surface-2)] text-[var(--rg-fg-muted)] cursor-not-allowed'}"
          onclick={save}>{saving ? $t('customTheme.saving') : $t('customTheme.save')}</button>
      </footer>
    </div>
  </div>
{/if}
```

> 依赖检查：`@tauri-apps/plugin-fs` 的 `readFile` 用于读图片字节。若项目未装该插件，改用 `openDialog` 返回路径后由后端读：把 `save_theme_bg_image` 改成接收路径参数版本，或新增 `save_theme_bg_image_from_path(path)`。执行时先 `grep -r "plugin-fs" package.json src-tauri/Cargo.toml`确认；未装则走「后端读路径」分支（见下方备注）。

- [ ] **Step 5.6: 校验 plugin-fs 是否可用，必要时改后端读路径**

Run: `grep -rn "plugin-fs\|tauri-plugin-fs" package.json src-tauri/Cargo.toml src-tauri/capabilities/ 2>/dev/null`

- 若**有** `plugin-fs` 且 capability 允许 `readFile`：保留 Step 5.5 写法。
- 若**没有**：在 `customTheme` 选图逻辑改为传路径给后端。具体：
  - ridge-core 加 `save_theme_bg_image_from_path(src: &str) -> CoreResult<String>`（读 `src` 字节→复用 `save_theme_bg_image` 校验与写入逻辑，扩展名取自 `src`）。
  - src-tauri 加 `#[tauri::command] save_theme_bg_image_from_path(path: String)` 封装并注册。
  - `themes.ts` 加 `saveThemeBgImageFromPath(path: string)` invoke 封装。
  - Modal 的 `pickImage` 改为：`form.bgImage = await saveThemeBgImageFromPath(picked);`（删掉 `readFile` 分支）。

- [ ] **Step 5.7: 类型检查**

Run: `pnpm check`
Expected: 无新增类型错误（如有 `plugin-fs` 未装报错，按 5.6 切换到后端读路径方案）。

- [ ] **Step 5.8: Commit**

```bash
git add src/lib/components/customTheme.ts src/lib/components/customTheme.test.ts src/lib/components/CustomThemeModal.svelte
git commit -m "feat(theme): CustomThemeModal 编辑弹窗 + 实时预览 + 纯逻辑单测

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: SettingsPanel 接入（创建卡 + 编辑/删除）+ i18n

**Files:**
- Modify: `src/lib/i18n/messages.ts`
- Modify: `src/lib/components/SettingsPanel.svelte`

- [ ] **Step 6.1: 加 i18n 文案（zh + en）**

在 `src/lib/i18n/messages.ts` 的 zh `settings` 对象内（`themeDesc` 之后）加：

```ts
    customThemeCard: '自定义主题',
    customThemeEdit: '编辑',
    customThemeDelete: '删除',
    customThemeDeleteConfirm: '确定删除这个自定义主题吗？',
```

并在 zh 顶层（与 `settings` 同级）新增 `customTheme` 段：

```ts
  customTheme: {
    title: '自定义主题',
    newTitle: '新建自定义主题',
    editTitle: '编辑自定义主题',
    name: '主题名称',
    namePlaceholder: '给你的主题起个名字',
    type: '类型',
    dark: '深色',
    light: '浅色',
    basedOn: '基于',
    bgImage: '终端背景图',
    chooseImage: '选择图片…',
    removeImage: '移除',
    bgOpacity: '背景图透明度',
    coreColors: '核心配色',
    advanced: '进阶（ANSI 终端色 / 启动动画色）',
    ansiColors: 'ANSI 终端色',
    loaderColors: '启动动画色',
    preview: '预览',
    save: '保存',
    saving: '保存中…',
  },
```

在 en `settings` 对象内（`themeDesc` 之后）加：

```ts
    customThemeCard: 'Custom Theme',
    customThemeEdit: 'Edit',
    customThemeDelete: 'Delete',
    customThemeDeleteConfirm: 'Delete this custom theme?',
```

并在 en 顶层新增 `customTheme` 段：

```ts
  customTheme: {
    title: 'Custom Theme',
    newTitle: 'New Custom Theme',
    editTitle: 'Edit Custom Theme',
    name: 'Theme name',
    namePlaceholder: 'Name your theme',
    type: 'Type',
    dark: 'Dark',
    light: 'Light',
    basedOn: 'Based on',
    bgImage: 'Terminal background',
    chooseImage: 'Choose image…',
    removeImage: 'Remove',
    bgOpacity: 'Background opacity',
    coreColors: 'Core colors',
    advanced: 'Advanced (ANSI / loader colors)',
    ansiColors: 'ANSI colors',
    loaderColors: 'Loader colors',
    preview: 'Preview',
    save: 'Save',
    saving: 'Saving…',
  },
```

> 若 `common.cancel` 不存在，于 `common` 段补 `cancel: '取消'` / `cancel: 'Cancel'`。执行时先 `grep -n "cancel" src/lib/i18n/messages.ts` 确认。

- [ ] **Step 6.2: SettingsPanel 接入弹窗与状态**

修改 `src/lib/components/SettingsPanel.svelte`：

(a) `<script>` import 区加：

```ts
  import { isCustomTheme, deleteCustomTheme } from '$lib/stores/themes';
  import { Pencil, Trash2, Plus } from 'lucide-svelte';
  import CustomThemeModal from './CustomThemeModal.svelte';
```

（`lucide-svelte` 已在用，合并到现有那行 import 亦可。）

(b) `let activeSection = ...` 附近加弹窗状态：

```ts
  let customModalOpen = $state(false);
  let customEditingId = $state<string | null>(null);

  function openNewCustomTheme(): void { customEditingId = null; customModalOpen = true; }
  function openEditCustomTheme(id: string): void { customEditingId = id; customModalOpen = true; }
  async function removeCustomTheme(id: string): Promise<void> {
    if (!confirm($t('settings.customThemeDeleteConfirm'))) return;
    const wasActive = $settingsStore.theme === id;
    await deleteCustomTheme(id);
    if (wasActive) setTheme('endless-dark');
  }
```

(c) appearance 段的主题网格（line 160-186 `grid grid-cols-2`）：在 `{#each themeIds as id (id)}` 块的现有 `<button>` 内，紧跟主题名那行（line 178-183 的 `<div class="px-3 py-2 ...">` 内 `<span>{themeLabels[id]}</span>` 之后）加编辑/删除入口；并在 `{/each}` 之后加创建卡。

把现有卡片内底部信息条改为（保留原 selected 徽标，追加自定义操作）：

```svelte
                    <div class="px-3 py-2 bg-[var(--rg-surface)]/60 flex items-center justify-between gap-1">
                      <span class="text-[12px] font-medium text-[var(--rg-fg)] truncate">{themeLabels[id]}</span>
                      <div class="flex items-center gap-1 shrink-0">
                        {#if selected}
                          <span class="text-[10px] px-1.5 py-0.5 rounded bg-[var(--rg-accent)]/20 text-[var(--rg-accent)] font-mono uppercase">使用中</span>
                        {/if}
                        {#if isCustomTheme(id)}
                          <button type="button" class="h-5 w-5 flex items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface-2)]"
                            title={$t('settings.customThemeEdit')}
                            onclick={(e) => { e.stopPropagation(); openEditCustomTheme(id); }}>
                            <Pencil class="h-3 w-3" />
                          </button>
                          <button type="button" class="h-5 w-5 flex items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-ansi-red,#e3342f)] hover:bg-[var(--rg-surface-2)]"
                            title={$t('settings.customThemeDelete')}
                            onclick={(e) => { e.stopPropagation(); void removeCustomTheme(id); }}>
                            <Trash2 class="h-3 w-3" />
                          </button>
                        {/if}
                      </div>
                    </div>
```

在 `{/each}` 之后、`</div>`（grid 收尾）之前加创建卡：

```svelte
                <button
                  type="button"
                  class="rounded-lg border-2 border-dashed border-[var(--rg-border)] hover:border-[var(--rg-accent)] hover:text-[var(--rg-accent)] text-[var(--rg-fg-muted)] flex flex-col items-center justify-center gap-1 min-h-[92px] transition-colors"
                  onclick={openNewCustomTheme}
                >
                  <Plus class="h-5 w-5" />
                  <span class="text-[12px]">{$t('settings.customThemeCard')}</span>
                </button>
```

(d) 在组件最外层 `{#if open}` 的根 `<div>` **同级**（即 SettingsPanel 根遮罩之后）挂弹窗，使其 z-index 叠在设置面板之上：在文件末尾 `{/if}`（line 419）之前、根遮罩 `</div>` 之后加：

```svelte
  <CustomThemeModal open={customModalOpen} editingId={customEditingId} onClose={() => (customModalOpen = false)} />
```

> 放在 SettingsPanel 的 `{#if open}` 之内但根遮罩 `</div>` 之外，保证设置面板开着时才可能弹；弹窗自身 z-9996 高于设置面板 9994。

- [ ] **Step 6.3: 类型检查**

Run: `pnpm check`
Expected: 无新增类型错误。

- [ ] **Step 6.4: 构建验证**

Run: `pnpm build`
Expected: 构建成功。

- [ ] **Step 6.5: Commit**

```bash
git add src/lib/components/SettingsPanel.svelte src/lib/i18n/messages.ts
git commit -m "feat(theme): 设置面板自定义主题创建卡 + 编辑/删除入口 + i18n

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## 收尾验证

- [ ] **全量测试**

Run: `cargo test -p ridge-core && pnpm test && pnpm check`
Expected: 全部 PASS / 无类型错误。

- [ ] **手动冒烟（在常驻 tauri dev 里）**

1. 打开设置 → 外观 → 点「+ 自定义主题」→ 命名、选背景图、调透明度与若干核心色 → 右侧预览实时变化 → 保存。
2. 新主题出现在列表并被选中；终端区域显示背景图（按透明度），文字清晰可读。
3. hover 自定义卡 → 编辑改色保存生效；删除（若正用→回退默认）。
4. 重启应用，自定义主题与选择保持（来自 user-themes.json + active-theme.txt）。

---

## Self-Review 记录

- **Spec 覆盖**：数据/持久化(Task1-2)、终端背景图无 Rust 改动(Task4)、编辑弹窗+预览(Task5)、创建卡+编辑/删除(Task6)、core 单测(Task1)、提交拆分对齐 spec §7。✓
- **占位符**：无 TBD/TODO；每个代码步给了完整代码。✓
- **类型一致**：`ThemeEntry.bgImage/bgImageOpacity`（TS）↔ `bg_image/bg_image_opacity`+serde rename（Rust）；`saveCustomTheme`/`deleteCustomTheme`/`saveThemeBgImage`/`setActiveBgImage`/`activeBgImage`/`isCustomTheme`/`slugifyThemeId` 在 Task3 定义、Task4-6 引用，命名一致；`CORE_COLOR_KEYS`(18)/`ANSI_COLOR_KEYS`(16)/`ALPHA_COLOR_KEYS`/`previewStyle`/`buildThemeEntry`/`ThemeFormState` 在 Task5 定义并自用。✓
- **风险点显式标注**：plugin-fs 依赖在 Step 5.6 给了后端读路径回退；RidgePane 背景层 z-index 在 Step 4.2 给了实测兜底说明。
