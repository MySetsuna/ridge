import { describe, expect, it } from 'vitest';
import * as commune from './messages/commune';
import * as workspace from './messages/workspace';

describe('localized workspace and Commune messages', () => {
  it('keeps English and Chinese keys in sync', () => {
    expect(Object.keys(commune.en).sort()).toEqual(Object.keys(commune.zh).sort());
    expect(Object.keys(workspace.en).sort()).toEqual(Object.keys(workspace.zh).sort());
  });
});
