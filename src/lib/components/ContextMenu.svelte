<script lang="ts">
	import { contextMenu, type ContextMenuItem } from '$lib/stores/contextMenu';
	import * as Icons from 'lucide-svelte';
	import { onMount } from 'svelte';

	let menuRef: HTMLDivElement;

	let state = $derived($contextMenu);

	function handleClick(item: ContextMenuItem) {
		if (!item.disabled) {
			item.action();
			contextMenu.hide();
		}
	}

	function handleClickOutside(event: MouseEvent) {
		if (menuRef && !menuRef.contains(event.target as Node)) {
			contextMenu.hide();
		}
	}

	function handleKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			contextMenu.hide();
		}
	}

	onMount(() => {
		document.addEventListener('click', handleClickOutside);
		document.addEventListener('keydown', handleKeydown);
		return () => {
			document.removeEventListener('click', handleClickOutside);
			document.removeEventListener('keydown', handleKeydown);
		};
	});

	function getIconComponent(iconName?: string) {
		if (!iconName) return null;
		const iconMap: Record<string, any> = Icons;
		return iconMap[iconName] || null;
	}
</script>

{#if state.visible}
	<div
		bind:this={menuRef}
		class="fixed z-[9999] min-w-[180px] overflow-hidden rounded-lg border border-[var(--wf-border)] bg-[var(--wf-surface)]/95 backdrop-blur-md shadow-[0_8px_32px_rgba(0,0,0,0.5)]"
		style="left: {state.x}px; top: {state.y}px;"
	>
		{#each state.items as item}
			{#if item.divider}
				<div class="my-1 border-t border-[var(--wf-border)]"></div>
			{:else}
				<button
					type="button"
					class="flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-[var(--wf-fg)] transition-colors hover:bg-[var(--wf-accent)]/20 disabled:opacity-40 disabled:pointer-events-none"
					disabled={item.disabled}
					onclick={() => handleClick(item)}
				>
					{#if item.icon}
						{@const IconComponent = getIconComponent(item.icon)}
						{#if IconComponent}
							<span class="flex h-4 w-4 items-center justify-center">
								<IconComponent size={14} strokeWidth={2} />
							</span>
						{/if}
					{/if}
					<span class="flex-1">{item.label}</span>
					{#if item.shortcut}
						<span class="text-xs text-[var(--wf-fg-muted)]">{item.shortcut}</span>
					{/if}
				</button>
			{/if}
		{/each}
	</div>
{/if}