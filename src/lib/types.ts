// src/lib/types.ts

/**
 * Teammate (Claude Code sub-agent) runtime state surfaced on a leaf pane.
 * Populated by the backend when `/api/v1/register-agent` or the Tauri
 * `register_teammate_agent` command binds an agent to the pane; null when
 * the pane has never been touched by teammate routing.
 */
export type AgentState = 'idle' | 'busy' | 'starting';

export type PaneNode =
  | {
      type: 'leaf';
      id: string;
      title?: string;
      cwd?: string;
      /** "idle" | "busy" | "starting" if teammate ever marked this pane. */
      agent_state?: AgentState;
      /** agent_id that currently owns this pane (when busy). */
      agent_id?: string;
    }
  | {
      type: 'split';
      id: string;
      direction: 'horizontal' | 'vertical';
      children: PaneNode[];
      ratios: number[];
    };