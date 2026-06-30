// src/lib/types.ts

/**
 * Teammate (Claude Code sub-agent) runtime state surfaced on a leaf pane.
 * Populated by the backend when `/api/v1/register-agent` or the Tauri
 * `register_teammate_agent` command binds an agent to the pane; null when
 * the pane has never been touched by teammate routing.
 */
export type AgentState = 'idle' | 'busy' | 'starting';

/**
 * 非本地来源的 pane —— 其底层 PTY 不归本地工作区持有，而是被某个「外部终端
 * provider」接管：领养的本地无头(native/headless)会话、或远端 ridge / rdg 主机
 * 的 pane。后端 `get_pane_layout` 仅在 pane 为外部来源时回传 `origin`；本地 pane
 * 省略该字段。字段使用 snake_case 以与后端 `LayoutNode` DTO 对齐（与 shell_kind /
 * agent_state 同一约定）。详见 docs/superpowers/specs/2026-06-30-...-hosts-design.md。
 */
export type PaneOrigin =
  | { kind: 'headless'; host_id: string; host_label: string; session_id: string }
  | { kind: 'remote'; host_id: string; host_label: string; session_id: string }
  | { kind: 'rdg'; host_id: string; host_label: string; session_id: string };

export type PaneNode =
  | {
      type: 'leaf';
      id: string;
      title?: string;
      cwd?: string;
      /** 本 pane 当前 shell 的 program（后端 get_pane_layout 回传，用于切换器标签）。 */
      shell_kind?: string;
      /** "idle" | "busy" | "starting" if teammate ever marked this pane. */
      agent_state?: AgentState;
      /** agent_id that currently owns this pane (when busy). */
      agent_id?: string;
      /** 外部来源标识（无头/远端/rdg）。缺省 = 本地终端，由本地工作区持有。 */
      origin?: PaneOrigin;
    }
  | {
      type: 'split';
      id: string;
      direction: 'horizontal' | 'vertical';
      children: PaneNode[];
      ratios: number[];
    };