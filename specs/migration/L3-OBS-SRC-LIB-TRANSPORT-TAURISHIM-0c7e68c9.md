---
id: L3-OBS-SRC-LIB-TRANSPORT-TAURISHIM-0c7e68c9
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/transport/tauriShim module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/transport/tauriShim/bridge.ts
  - src/lib/transport/tauriShim/clipboard.ts
  - src/lib/transport/tauriShim/core.ts
  - src/lib/transport/tauriShim/dialog.ts
  - src/lib/transport/tauriShim/event.ts
  - src/lib/transport/tauriShim/opener.ts
  - src/lib/transport/tauriShim/window.ts
public_interface:
  - "export async function ask(msg: string): Promise<boolean>"
  - "export async function confirm(msg: string): Promise<boolean>"
  - "export async function emit(_event: string, _payload?: unknown):
    Promise<void>"
  - "export async function emitTo( _target: string, _event: string, _payload?:
    unknown, ): Promise<void>"
  - "export async function message(msg: string): Promise<void>"
  - "export async function open( options: OpenOptions = {}, ): Promise<string |
    string[] | null>"
  - "export async function openPath(path: string, _openWith?: string):
    Promise<void>"
  - "export async function openUrl(url: string | URL, _openWith?: string):
    Promise<void>"
  - "export async function readText(): Promise<string>"
  - "export async function revealItemInDir(path: string | string[]):
    Promise<void>"
  - "export async function save(options: { defaultPath?: string; title?: string
    } = {}): Promise<string | null>"
  - "export async function writeText(text: string): Promise<void>"
  - export class Channel
  - export class TauriBridge
  - "export function convertFileSrc(filePath: string, _protocol = 'asset'):
    string"
  - "export function getCurrent(): ShimWindow"
  - "export function getCurrentWindow(): ShimWindow"
  - "export function isTauri(): boolean"
  - "export function transformCallback(_callback?: (response: unknown)"
  - export interface TauriEvent
  - export type Event
  - export type EventCallback
  - export type UnlistenFn
---

# src/lib/transport/tauriShim module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
