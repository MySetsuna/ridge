// @ridge/remote — 统一远控前端包（设计见
// docs/superpowers/specs/2026-07-16-remote-frontend-unify-and-mobile-keepalive-design.md）
//
// 边界护栏：本包 **不得** import 主 app（`$lib/*` / `$app/*` / `src/*`）。
// 主 app 特有状态（settings / cwd / wallpaper）一律经端口注入（SettingsPort /
// CwdPort，见 §4.1）。依赖方向永远是 主 app → @ridge/remote，绝不反向。
//
// P1 进行中：传输原语（L1/L2 契约类型 + JSON-RPC + cloud 帧编解码）已迁入
// shared/transport，从此处桶 re-export 主 app 需要的公共面（裸导入 @ridge/remote，
// 与 @ridge/split 同机制，无需 alias）。后续 P2 迁 shared/terminal、P5 迁 mobile/panel。
export * from './shared/transport/types';
export * from './shared/transport/jsonRpc';
export * from './shared/transport/rpcClient';
export * from './shared/transport/cloudMux';
export * from './shared/transport/cloudChunk';
