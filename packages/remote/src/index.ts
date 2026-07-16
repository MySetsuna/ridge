// @ridge/remote — 统一远控前端包（设计见
// docs/superpowers/specs/2026-07-16-remote-frontend-unify-and-mobile-keepalive-design.md）
//
// 边界护栏：本包 **不得** import 主 app（`$lib/*` / `$app/*` / `src/*`）。
// 主 app 特有状态（settings / cwd / wallpaper）一律经端口注入（SettingsPort /
// CwdPort，见 §4.1）。依赖方向永远是 主 app → @ridge/remote，绝不反向。
//
// P0 骨架：空桶占位。后续 P1 迁 shared/transport、P2 迁 shared/terminal、
// P5 迁 mobile/ + panel/，再从此处 re-export 主 app 需要的公共面。

export {};
