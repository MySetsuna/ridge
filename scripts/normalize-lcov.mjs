#!/usr/bin/env node
// Sonar's LCOV importer expects repository-relative POSIX paths on Windows.

import fs from 'node:fs';
import path from 'node:path';

const reportPath = path.resolve(process.env.LCOV_PATH || 'coverage/lcov.info');
const report = fs.readFileSync(reportPath, 'utf8');
const normalized = report.replace(/^SF:(.*)$/gm, (_, file) => `SF:${file.replaceAll('\\', '/')}`);

if (normalized !== report) fs.writeFileSync(reportPath, normalized);
console.log(JSON.stringify({ ok: true, report: reportPath, changed: normalized !== report }));
