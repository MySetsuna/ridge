// src/lib/terminal/hostPorts.ts
//
// 主 app 侧 HostPorts 实现：把 manager / themeBridge（现居 @ridge/remote/shared/
// terminal）对主 app 的运行时依赖包装成端口，app 启动时经
// TerminalManager.setHostPorts(makeHostPorts()) 注入。行为与迁移前逐字守恒：
//   - settings.get/subscribe ← settingsStore（scrollback + fontFamily + 变更订阅）
//   - termSettings           ← termFontSize store（字号 + 变更订阅）
//   - themes                 ← activeBgImage（背景图 URL + 变更订阅）
//   - cwd.current/all        ← paneCwdStore
//   - openTextLink           ← 动态 import('$lib/utils/linkResolver')（保留懒加载，
//                              把 linkResolver + monaco 传递依赖留在 app 侧、离开包图）

import { get } from 'svelte/store';
import { settingsStore } from '$lib/stores/settings';
import { termFontSize } from '$lib/stores/termSettings';
import { activeBgImage } from '$lib/stores/themes';
import { paneCwdStore, activeWorkspaceId } from '$lib/stores/paneTree';
import type { HostPorts } from '@ridge/remote/shared/terminal/ports';

export function makeHostPorts(): HostPorts {
	return {
		settings: {
			get() {
				const s = get(settingsStore);
				return {
					terminalScrollbackLines: s.terminalScrollbackLines,
					terminalFontFamily: s.terminalFontFamily,
					defaultShell: s.defaultShell,
				};
			},
			subscribe(cb) {
				// Svelte store：订阅即同步触发一次，themeBridge 依赖此初次推送。
				let lastKey: string | undefined;
				return settingsStore.subscribe((s) => {
					const key = `${s.terminalScrollbackLines}\u0000${s.terminalFontFamily}\u0000${s.defaultShell}`;
					if (key === lastKey) return;
					lastKey = key;
					cb({
						terminalScrollbackLines: s.terminalScrollbackLines,
						terminalFontFamily: s.terminalFontFamily,
						defaultShell: s.defaultShell,
					});
				});
			},
		},
		workspace: {
			activeId() {
				return get(activeWorkspaceId);
			},
		},
		termSettings: {
			fontSize() {
				return get(termFontSize);
			},
			subscribe(cb) {
				return termFontSize.subscribe(cb);
			},
		},
		themes: {
			activeBgImageUrl() {
				return get(activeBgImage).url;
			},
			subscribe(cb) {
				return activeBgImage.subscribe(() => cb());
			},
		},
		cwd: {
			current(workspaceId, paneId) {
				return get(paneCwdStore)[`${workspaceId}:${paneId}`];
			},
			all() {
				return Object.values(get(paneCwdStore)).filter((s): s is string => !!s);
			},
		},
		openTextLink(spanText, ctx) {
			// §1.32：动态 import 保持 linkResolver（及其 monaco 传递依赖）不进
			// 调用方的加载图；点击容忍这一 microtask。
			void import('$lib/utils/linkResolver').then(({ resolveLink, executeAction }) => {
				const action = resolveLink(spanText, { cwd: ctx.cwd, knownCwds: ctx.knownCwds });
				void executeAction(action);
			});
		},
	};
}
