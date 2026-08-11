# Wave 87：非整数 DPR 栅格对齐与 ridge-term 质量收口

## 结果

- `packages/ridge-term` 新增 `snap_css_to_device`：Canvas2D 以设备像素为坐标吸附后再换回 CSS 像素，避免 DPR `1.25` / `1.5` 下边框、背景与相邻 cell 落在半设备像素。
- Canvas2D 的 cell 背景、文本、procedural box glyph、cursor、selection、超链接下划线统一走同一吸附路径；WebGPU 原有 device-pixel cell geometry 保持不变。
- `ridge-term` 既有 clippy 等价清理一并收口；未扩大协议或 PTY 语义。

## 证据

| 检查 | 结果 |
|---|---:|
| `cargo clippy -p ridge-term --all-targets -- -D warnings` | 通过 |
| `cargo test -p ridge-term --all-targets --quiet` | 通过 |
| `cargo check -p ridge-term --target wasm32-unknown-unknown --features webgpu --quiet` | 通过 |
| DPR 纯逻辑测试 | `1.25` / `1.5` / 无效 DPR 回退覆盖 |
| CodeGraph | `Canvas2dBackend::draw_row_texts → snap → snap_css_to_device` 已连通 |

## 边界

本波证明设备像素映射逻辑与 WASM 编译成立；原生 PowerShell 对照截图、实体设备 DPR/缩放矩阵仍需现场证据，不能由本地单测替代。

本波提交：`6699c96c`。凭据、Cookie、Sonar token、NLM Cookie 未写入本文档或仓库。
