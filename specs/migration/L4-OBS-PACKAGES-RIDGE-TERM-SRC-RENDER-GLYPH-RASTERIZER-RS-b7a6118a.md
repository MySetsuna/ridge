---
id: L4-OBS-PACKAGES-RIDGE-TERM-SRC-RENDER-GLYPH-RASTERIZER-RS-b7a6118a
level: L4
parent: L3-OBS-PACKAGES-RIDGE-TERM-SRC-RENDER-0f92aefb
title: glyph_rasterizer.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-term/src/render/glyph_rasterizer.rs
---

# glyph_rasterizer.rs

WASM WebGPU 字形圖集須支援兩種確定性來源：已註冊 Host 字體時沿用 cosmic-text/Swash；未註冊 Host 字體之 Remote Web 使用隱藏 Canvas2D，交由控制端瀏覽器與作業系統解析 CSS 系統字體、CJK、符號及 emoji。Canvas 僅生成圖集像素，不作終端呈現後端；不得讀取或傳輸本機字體檔。
