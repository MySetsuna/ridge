#!/usr/bin/env node
// Sonar's LCOV importer expects repository-relative POSIX paths on Windows.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function normalizeLcov(report) {
  return report.replace(/^SF:(.*)$/gm, (_, file) => `SF:${file.replaceAll('\\', '/')}`);
}

export function main(reportPath = path.resolve(process.env.LCOV_PATH || 'coverage/lcov.info'), io = console) {
  const report = fs.readFileSync(reportPath, 'utf8');
  const normalized = normalizeLcov(report);
  const changed = normalized !== report;
  if (changed) fs.writeFileSync(reportPath, normalized);
  io.log(JSON.stringify({ ok: true, report: reportPath, changed }));
  return { ok: true, report: reportPath, changed };
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  main();
}
