import type { DataProvider } from './types';

let current: DataProvider | null = null;

export function setTransport(provider: DataProvider): void {
  current = provider;
}

export function getTransport(): DataProvider {
  if (!current) throw new Error('No DataProvider registered. Call setTransport() first.');
  return current;
}

export function hasTransport(): boolean {
  return current !== null;
}