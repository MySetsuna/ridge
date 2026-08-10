/** Return a cryptographically backed unit interval when Web Crypto exists. */
export function secureRandomUnit(): number {
  const values = new Uint32Array(1);
  const cryptoApi = globalThis.crypto;
  if (!cryptoApi?.getRandomValues) return 0.5;
  try {
    cryptoApi.getRandomValues(values);
    return values[0] / (2 ** 32);
  } catch {
    return 0.5;
  }
}
