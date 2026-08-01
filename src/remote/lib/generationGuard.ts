export function createGenerationGuard() {
  let current = 0;
  return {
    begin: () => ++current,
    invalidate: () => { current += 1; },
    isCurrent: (generation: number) => generation === current,
  };
}
