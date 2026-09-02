---
id: L4-OBS-SRC-LIB-LSP-LSPCLIENT-TS-200ded4f
level: L4
parent: L3-OBS-SRC-LIB-LSP-2e495523
title: lspClient.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/lsp/lspClient.ts
test_targets:
  - src/lib/lsp/lspClient.test.ts
public_interface:
  - "export async function lspDefinition( workspaceRoot: string, path: string,
    line: number, character: number ): Promise<LspTarget[]>"
  - "export async function lspDidChange( workspaceRoot: string, path: string,
    version: number, text: string ): Promise<void>"
  - "export async function lspDidOpen( workspaceRoot: string, path: string,
    text: string ): Promise<void>"
  - "export async function lspHover( workspaceRoot: string, path: string, line:
    number, character: number ): Promise<LspHover | null>"
  - "export async function lspReferences( workspaceRoot: string, path: string,
    line: number, character: number ): Promise<LspTarget[]>"
  - "export function classifyLspError(err: unknown):"
  - "export function lspLanguageId(path: string): string | null"
  - "export function lspSupports(path: string): boolean"
  - "export function notifyLspError(err: unknown): boolean"
  - "export function onLspDiagnostics( cb: (payload: LspDiagnosticsPayload)"
  - "export function parseDefinition(raw: unknown): LspTarget[]"
  - "export function parseHover(raw: unknown): LspHover | null"
  - "export function pathToUri(path: string): string"
  - "export function resetLspErrorNotices(): void"
  - "export function setLspErrorSink(sink: (hint: string)"
  - "export function uriToPath(uri: string): string"
  - export interface LspDiagnostic
  - export interface LspDiagnosticsPayload
  - export interface LspHover
  - export interface LspTarget
verified_by:
  - TEST-OBS-SRC-LIB-LSP-LSPCLIENT-TEST-TS-bab87361
---

# lspClient.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
