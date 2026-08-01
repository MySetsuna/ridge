export type RepeatedErrorLevel = 'debug' | 'info' | 'warn' | 'error';

const DEFAULT_WINDOW_MS = 5_000;

interface RepeatedErrorEntry {
  count: number;
  label: string;
  detail: string;
  level: RepeatedErrorLevel;
  timer: ReturnType<typeof setTimeout>;
}

const repeatedErrors = new Map<string, RepeatedErrorEntry>();

function errorDetail(error: unknown): string {
  if (error instanceof Error) return `${error.name}: ${error.message}`;
  if (typeof error === 'string') return error;
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function classifyLevel(detail: string, fallback: RepeatedErrorLevel): RepeatedErrorLevel {
  return /pane not found|not a git repository/i.test(detail) ? 'warn' : fallback;
}

function emit(level: RepeatedErrorLevel, ...args: unknown[]): void {
  const logger = console[level];
  if (typeof logger === 'function') logger(...args);
}

/**
 * Preserve the first failure, then compress an identical burst into one timed
 * summary. Callers opt in explicitly; this never patches or filters Console.
 */
export function reportRepeatedError(
  label: string,
  error: unknown,
  fallbackLevel: RepeatedErrorLevel = 'error',
  windowMs = DEFAULT_WINDOW_MS,
): void {
  const detail = errorDetail(error);
  const level = classifyLevel(detail, fallbackLevel);
  const key = `${level}\0${label}\0${detail}`;
  const existing = repeatedErrors.get(key);
  if (existing) {
    existing.count += 1;
    return;
  }

  emit(level, label, error);
  const entry: RepeatedErrorEntry = {
    count: 0,
    label,
    detail,
    level,
    timer: setTimeout(() => {
      repeatedErrors.delete(key);
      if (entry.count > 0) {
        emit(entry.level, `${entry.label} (${entry.detail}), repeated ${entry.count} times`);
      }
    }, windowMs),
  };
  repeatedErrors.set(key, entry);
}

/** Test/HMR cleanup; normal entries self-release after their summary window. */
export function clearRepeatedErrors(): void {
  for (const entry of repeatedErrors.values()) clearTimeout(entry.timer);
  repeatedErrors.clear();
}
