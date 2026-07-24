/**
 * AC4-C9 product path: matrix ↔ allowlist ↔ REMOTE_CAPABILITY_METHODS parity.
 * Used by vitest + scripts/check-capability-matrix.mjs philosophy (TS side).
 */

import { REMOTE_CAPABILITY_METHODS } from './capabilityContract';
import {
  DESKTOP_PRIVILEGED_METHODS,
  TEAMMATE_REMOTE_REQUIRED,
  forbiddenPresent,
  missingRequired,
} from './protocolAdmission';

export interface MatrixDoc {
  capabilities?: Record<string, { methods?: string[] }>;
}

export interface ParityReport {
  ok: boolean;
  teammateMissing: string[];
  teammateLeaks: string[];
  controllerMissing: string[];
  issues: string[];
}

/** Teammate methods required on both matrix and TS controller contract. */
export function teammateFromMatrix(matrix: MatrixDoc): string[] {
  const m = matrix.capabilities?.teammate?.methods;
  return Array.isArray(m) ? m.filter((x) => typeof x === 'string') : [];
}

export function allMatrixMethods(matrix: MatrixDoc): string[] {
  const caps = matrix.capabilities ?? {};
  const out: string[] = [];
  for (const cap of Object.values(caps)) {
    if (Array.isArray(cap?.methods)) {
      for (const m of cap.methods) {
        if (typeof m === 'string') out.push(m);
      }
    }
  }
  return out;
}

/**
 * Full parity check for product gates.
 * @param allowlist remote allowlist method names
 * @param matrix capability matrix document
 */
export function reportMatrixParity(
  allowlist: readonly string[],
  matrix: MatrixDoc,
): ParityReport {
  const issues: string[] = [];
  const allow = [...allowlist];
  const teammateMatrix = teammateFromMatrix(matrix);

  const teammateMissing = missingRequired(teammateMatrix, TEAMMATE_REMOTE_REQUIRED);
  if (teammateMissing.length) {
    issues.push(`matrix teammate missing: ${teammateMissing.join(',')}`);
  }

  const teammateLeaks = forbiddenPresent(teammateMatrix, DESKTOP_PRIVILEGED_METHODS);
  if (teammateLeaks.length) {
    issues.push(`matrix teammate leaks desktop: ${teammateLeaks.join(',')}`);
  }

  // Controller contract teammate methods must be in allowlist
  const controllerTeam = [...REMOTE_CAPABILITY_METHODS.teammate];
  const controllerMissing = missingRequired(allow, controllerTeam);
  if (controllerMissing.length) {
    issues.push(`allowlist missing controller teammate: ${controllerMissing.join(',')}`);
  }

  // Desktop privileged must not be in allowlist
  const allowLeaks = forbiddenPresent(allow, DESKTOP_PRIVILEGED_METHODS);
  if (allowLeaks.length) {
    issues.push(`allowlist leaks desktop: ${allowLeaks.join(',')}`);
  }

  return {
    ok: issues.length === 0,
    teammateMissing,
    teammateLeaks,
    controllerMissing,
    issues,
  };
}

/** Whether a panel capability's methods are fully admitted. */
export function capabilityFullyAdmitted(
  capability: keyof typeof REMOTE_CAPABILITY_METHODS,
  allowlist: readonly string[],
): boolean {
  const need = REMOTE_CAPABILITY_METHODS[capability];
  return need.every((m) => allowlist.includes(m));
}

export function deniedControllerMethods(
  capability: keyof typeof REMOTE_CAPABILITY_METHODS,
  allowlist: readonly string[],
): string[] {
  return REMOTE_CAPABILITY_METHODS[capability].filter((m) => !allowlist.includes(m));
}
