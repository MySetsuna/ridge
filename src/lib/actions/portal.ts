// src/lib/actions/portal.ts
//
// Svelte 5 action that physically moves the host element into another DOM
// container (default: <body>). Used for popovers/dropdowns so they escape
// scrolled/clipped/transformed ancestor stacking contexts.

import type { Action } from 'svelte/action';

export interface PortalOptions {
	target?: HTMLElement | string;
	id?: string;
}

export const portal: Action<HTMLElement, PortalOptions | undefined> = (node, options: PortalOptions | undefined) => {
	if (typeof document === 'undefined') return { destroy() {} };

	function resolveTarget(opt: PortalOptions | undefined): HTMLElement {
		const t = opt?.target;
		if (!t) return document.body;
		if (typeof t === 'string') {
			const el = document.querySelector(t);
			if (!(el instanceof HTMLElement)) {
				throw new Error(`portal target "${t}" not found`);
			}
			return el;
		}
		return t;
	}

	function move(opt: PortalOptions | undefined): void {
		const tgt = resolveTarget(opt);
		if (node.parentElement === tgt) return;
		if (opt?.id) node.dataset.rgPortalId = opt.id;
		tgt.appendChild(node);
	}

	move(options);
	return {
		update(o) {
			move(o);
		},
		destroy() {
			try {
				node.remove();
			} catch {
				/* node already detached */
			}
		},
	};
};
