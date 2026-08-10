/**
 * Workspace goal/constraint/task memory bridge.
 *
 * The Tauri sidecar is the durable projection. This module keeps its wire
 * shape bounded and makes non-Tauri/Remote callers fail closed to an empty
 * snapshot instead of leaking an IPC error into workspace switching.
 */

export interface WorkspaceMemory {
  goal: string;
  constraints: string[];
  tasks: unknown[];
  updatedAt?: number;
}

export function emptyWorkspaceMemory(): WorkspaceMemory {
  return { goal: '', constraints: [], tasks: [] };
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/** Parse untrusted IPC data without allowing malformed memory to poison state. */
export function parseWorkspaceMemory(value: unknown): WorkspaceMemory {
  const source = record(value);
  if (!source) return emptyWorkspaceMemory();

  const goal = typeof source.goal === 'string' ? source.goal : '';
  const constraints = Array.isArray(source.constraints)
    ? source.constraints.filter((item): item is string => typeof item === 'string')
    : [];
  const tasks = Array.isArray(source.tasks) ? source.tasks : [];
  const updatedAt =
    typeof source.updatedAt === 'number' && Number.isFinite(source.updatedAt) && source.updatedAt >= 0
      ? source.updatedAt
      : undefined;

  return updatedAt === undefined
    ? { goal, constraints, tasks }
    : { goal, constraints, tasks, updatedAt };
}

/** Read the desktop sidecar; browser/SSR/old hosts degrade to an empty view. */
export async function loadWorkspaceMemory(workspaceId: string | undefined): Promise<WorkspaceMemory> {
  if (!workspaceId) return emptyWorkspaceMemory();
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    return parseWorkspaceMemory(await invoke('get_workspace_memory', { workspaceId }));
  } catch {
    return emptyWorkspaceMemory();
  }
}

/** Persist a complete bounded projection; empty strings/lists clear their sections. */
export async function saveWorkspaceMemory(
  workspaceId: string | undefined,
  memory: WorkspaceMemory,
): Promise<boolean> {
  if (!workspaceId) return false;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('set_workspace_memory', {
      workspaceId,
      goal: memory.goal,
      constraints: memory.constraints,
      tasks: memory.tasks,
    });
    return true;
  } catch {
    return false;
  }
}
