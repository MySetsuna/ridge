export type ExplorerPasteOutcome =
  | { status: 'succeeded'; source: string; target: string }
  | { status: 'failed'; source: string; target: string; error: string };

export interface ExplorerPasteFailure {
  source: string;
  target: string;
  error: string;
}

export interface ExplorerPasteSummary {
  status: 'succeeded' | 'partial' | 'failed';
  attempted: number;
  succeeded: number;
  failed: number;
  failedPaths: string[];
  failures: ExplorerPasteFailure[];
}

export function summarizeExplorerPaste(outcomes: readonly ExplorerPasteOutcome[]): ExplorerPasteSummary {
  const failures = outcomes.flatMap((outcome) =>
    outcome.status === 'failed'
      ? [{ source: outcome.source, target: outcome.target, error: outcome.error }]
      : [],
  );
  const succeeded = outcomes.length - failures.length;
  const status = failures.length === 0 ? 'succeeded' : succeeded === 0 ? 'failed' : 'partial';

  return {
    status,
    attempted: outcomes.length,
    succeeded,
    failed: failures.length,
    failedPaths: failures.map(({ source }) => source),
    failures,
  };
}
