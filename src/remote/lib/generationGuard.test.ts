import { describe, expect, it } from 'vitest';
import { createGenerationGuard } from './generationGuard';

describe('generation guard', () => {
  it('supersedes older async work', () => {
    const guard = createGenerationGuard();
    const first = guard.begin();
    const second = guard.begin();

    expect(guard.isCurrent(first)).toBe(false);
    expect(guard.isCurrent(second)).toBe(true);
  });

  it('invalidates work during teardown', () => {
    const guard = createGenerationGuard();
    const generation = guard.begin();

    guard.invalidate();

    expect(guard.isCurrent(generation)).toBe(false);
  });
});
