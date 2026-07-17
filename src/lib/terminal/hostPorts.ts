// src/lib/terminal/hostPorts.ts
//
// 主 app 侧 HostPorts 实现：把 manager（现居 @ridge/remote/shared/terminal）对
// 主 app 的运行时依赖（终端设置 / pane cwd / 链接点击路由）包装成端口，app 启动
// 时经 TerminalManager.setHostPorts(makeHostPorts()) 注入。行为与迁移前逐字守恒：
//   - settings.get()  ← 原 manager L1230 get(settingsStore)
//   - cwd.current/all ← 原 static _currentPaneCwd/_knownCwds 的 get(paneCwdStore)
//   - openTextLink    ← 原 L1505 动态 import('$lib/utils/linkResolver')（保留懒加载，
//                        把 linkResolver + monaco 传递依赖留在 app 侧、离开 manager 图）

import { get } from 'svelte/store';
import { settingsStore } from '$lib/stores/settings';
import { paneCwdStore } from '$lib/stores/paneTree';
import type { HostPorts } from '@ridge/remote/shared/terminal/ports';

export function makeHostPorts(): HostPorts {
	return {
		settings: {
			get() {
				const s = get(settingsStore);
				return { terminalScrollbackLines: s.terminalScrollbackLines };
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
