import { describe, expect, it } from 'vitest';
import { emptyWorkspaceMemory, parseWorkspaceMemory } from './workspaceMemory';

describe('workspace memory wire boundary', () => {
  it('normalizes missing and malformed data to an empty snapshot', () => {
    expect(parseWorkspaceMemory(null)).toEqual(emptyWorkspaceMemory());
    expect(parseWorkspaceMemory({ goal: 42, constraints: ['keep', 3], tasks: 'bad' })).toEqual({
      goal: '',
      constraints: ['keep'],
      tasks: [],
    });
  });

  it('keeps valid goal, constraints, tasks, and timestamp', () => {
    const task = { id: 't1', status: 'open' };
    expect(
      parseWorkspaceMemory({
        goal: 'ship recovery',
        constraints: ['no force push'],
        tasks: [task],
        updatedAt: 12,
      }),
    ).toEqual({
      goal: 'ship recovery',
      constraints: ['no force push'],
      tasks: [task],
      updatedAt: 12,
    });
  });

  it('drops an invalid timestamp without dropping the rest of the snapshot', () => {
    expect(parseWorkspaceMemory({ goal: 'x', updatedAt: -1 })).toEqual({
      goal: 'x',
      constraints: [],
      tasks: [],
    });
  });
});
