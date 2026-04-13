// src/lib/types.ts
export type PaneNode =
  | { type: 'leaf'; id: string }
  | {
      type: 'split';
      id: string;
      direction: 'horizontal' | 'vertical';
      children: PaneNode[];
      ratios: number[];
    };