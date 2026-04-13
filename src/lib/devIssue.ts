import { writable } from 'svelte/store';

/** 开发模式下 Next.js 风格 issue 弹窗的数据 */
export type DevIssuePayload = {
  title?: string;
  message: string;
  stack?: string;
};

export const devIssue = writable<DevIssuePayload | null>(null);

export function reportDevIssue(payload: DevIssuePayload): void {
  if (import.meta.env.PROD) return;
  devIssue.set({
    title: payload.title ?? 'Unhandled Runtime Error',
    message: payload.message,
    stack: payload.stack
  });
}

export function clearDevIssue(): void {
  devIssue.set(null);
}
