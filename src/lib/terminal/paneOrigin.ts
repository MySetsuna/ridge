// src/lib/terminal/paneOrigin.ts
//
// 外部来源 pane 的标识徽标渲染助手（DRY）：工作区 pane 头部（SplitContainer）与
// Explorer/侧边栏的 pane chip 共用，保证「HEADLESS / LAN / rdg」徽标在两处一致。
// 配色沿用 pane 头部既有 AGENT/STARTING pill 的 token 风格。
import type { PaneOrigin } from '$lib/types';

export interface PaneOriginBadge {
  /** 显示在头部胶囊里的短标签。 */
  label: string;
  /** 胶囊配色类（与 AGENT/STARTING pill 同风格：`bg-…/15 text-…/300 border-…/40`）。 */
  pillClass: string;
  /** tooltip：解释「关闭=断开、真正终止在主机面板」。 */
  title: string;
}

/** pane 是否为外部来源（非本地工作区持有）。关闭=detach 的语义分流据此判定。 */
export function isForeignOrigin(origin: PaneOrigin | undefined): origin is PaneOrigin {
  return origin != null;
}

const CLOSE_HINT = '关闭仅断开，真正终止请在「主机」面板';

export function paneOriginBadge(origin: PaneOrigin): PaneOriginBadge {
  switch (origin.kind) {
    case 'headless':
      return {
        label: 'HEADLESS',
        pillClass: 'bg-slate-500/15 text-slate-300 border-slate-400/40',
        title: `本机无头会话 · ${CLOSE_HINT}`,
      };
    case 'remote':
      return {
        label: origin.host_label || 'LAN',
        pillClass: 'bg-sky-500/15 text-sky-300 border-sky-400/40',
        title: `来自远端主机 ${origin.host_label} · ${CLOSE_HINT}`,
      };
    case 'rdg':
      return {
        label: origin.host_label || 'rdg',
        pillClass: 'bg-violet-500/15 text-violet-300 border-violet-400/40',
        title: `来自 rdg 主机 ${origin.host_label} · ${CLOSE_HINT}`,
      };
  }
}
