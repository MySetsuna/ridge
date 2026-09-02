---
id: L3-OBS-SRC-LIB-UTILS-2c402078
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/utils module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/utils/anchorRect.ts
  - src/lib/utils/ansi.ts
  - src/lib/utils/linkResolver.ts
  - src/lib/utils/linkTrust.ts
  - src/lib/utils/markdown.ts
  - src/lib/utils/path.ts
  - src/lib/utils/pathToken.ts
  - src/lib/utils/pLimit.ts
  - src/lib/utils/repeatedError.ts
  - src/lib/utils/resizeThrottle.ts
  - src/lib/utils/withTimeout.ts
public_interface:
  - "export async function executeAction(action: LinkAction): Promise<void>"
  - "export async function highlightCodeBlocks(container: HTMLElement):
    Promise<void>"
  - "export async function openTerminalLink( request: TerminalLinkOpenRequest,
    dependencies: TerminalLinkDependencies = {}, ):
    Promise<TerminalLinkOpenResult>"
  - "export async function renderMermaidBlocks(container: HTMLElement):
    Promise<void>"
  - "export function _resetTrustedHosts_forTests(): void"
  - export function cancelThrottledResize()
  - "export function classifyLink(href: string): LinkKind"
  - "export function clearRepeatedErrors(): void"
  - "export function commonPathAncestor(paths: readonly string[]): string | null"
  - "export function hostKeyFromUrl(url: string): string | null"
  - "export function isCurrentDirHref(href: string): boolean"
  - "export function isExternalUrl(href: string): boolean"
  - "export function isHomeRelative(href: string): boolean"
  - "export function isMarkdownPath(path: string): boolean"
  - "export function isPosixAbsolute(href: string): boolean"
  - "export function isTrustedUrl(url: string, basePath?: string): boolean"
  - "export function isWindowsAbsolute(href: string): boolean"
  - "export function joinPath(base: string, rel: string): string"
  - "export function normalizePath(p: string): string"
  - "export function pathStartsWith(child: string, parent: string): boolean"
  - "export function pathTokenAt(lineContent: string, column: number): PathToken
    | null"
  - "export function popupStyleFor( anchor: HTMLElement, placement: Placement =
    'bottom-end', gap: number = 4, ): string"
  - "export function recommendedGitConcurrency(): number"
  - "export function renderMarkdown(source: string): string"
  - "export function reportRepeatedError( label: string, error: unknown,
    fallbackLevel: RepeatedErrorLevel = 'error', windowMs = DEFAULT_WINDOW_MS,
    ): void"
  - "export function resolveLink(href: string, ctx: ResolveCtx): LinkAction"
  - "export function stripAnsi(s: string): string"
  - "export function stripFrontMatter(source: string): string"
  - "export function stripQuery(pathPart: string): string"
  - "export function throttledUpdateResize(pointer: { x: number; y: number },
    callback: (pointer: { x: number; y: number })"
  - "export function toggleTaskAtLine(source: string, lineIndex: number): string"
  - "export function trimTrailingSeparators(value: string): string"
  - "export function trustHostFromUrl(url: string, basePath?: string): void"
  - export interface MapLimitOptions
  - export interface PathToken
  - export interface ResolveCtx
  - export interface TerminalLinkDependencies
  - export type LinkAction
  - export type LinkKind
  - export type Placement
  - export type RepeatedErrorLevel
---

# src/lib/utils module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
