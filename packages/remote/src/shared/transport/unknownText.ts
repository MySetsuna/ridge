/** Convert protocol diagnostics without coercing arbitrary objects to `[object Object]`. */
export function unknownText(value: unknown, fallback = ''): string {
  if (typeof value === 'string') return value;
  if (value instanceof Error) return value.message;
  if (value === null || value === undefined) return fallback;
  if (typeof value === 'object') {
    try {
      return JSON.stringify(value) ?? fallback;
    } catch {
      return fallback;
    }
  }
  if (typeof value === 'number' || typeof value === 'boolean' || typeof value === 'bigint') {
    return value.toString();
  }
  if (typeof value === 'symbol') return value.description ?? fallback;
  return fallback;
}
