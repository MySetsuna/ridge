// @ridge/remote — 统一远控前端包
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
export * from './shared/transport/paneRpcScheduler';
export * from './shared/transport/capabilityContract';
export * from './shared/transport/cloudMux';
export * from './shared/transport/cloudChunk';
// WS 原语层：LAN WebSocket 连接类 RemoteConnection（+ 契约类型 RemoteLink/
// PaneInfo/WorkspaceInfo/ConnectionState…）与设备身份 deviceId。手机壳、桌面
// +layout、transport/cloud 各 leg 共用；lanWsAdapter 即包装 RemoteConnection。
export * from './shared/transport/wsRemote';
export * from './shared/transport/deviceId';
// L1 适配器：LAN-WS(lanWsAdapter，包装 RemoteConnection + legacy 方言翻译)与
// cloud WebRTC(cloudWebrtcAdapter)；connectionProvider 为 cloud leg 的连接契约。
// 至此 transport/remote 整目录已并入本包，src/lib/transport/remote 清空。
export * from './shared/transport/connectionProvider';
export * from './shared/transport/lanWsAdapter';
export * from './shared/transport/cloudWebrtcAdapter';
