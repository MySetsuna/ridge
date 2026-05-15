<script lang="ts">
    import { terminalHistoryStore } from '$lib/stores/terminalHistory';
    import { onMount } from 'svelte';

    interface Props {
        query: string;
        isVisible: boolean;
        onSelect: (command: string) => void;
        onClose: () => void;
        position: { x: number, y: number };
    }

    let { query, isVisible, onSelect, onClose, position }: Props = $props();

    let filteredHistory = $derived($terminalHistoryStore.filter(h => h.toLowerCase().includes(query.toLowerCase())));
    let selectedIndex = $state(0);

    export function handleKeyDown(e: KeyboardEvent) {
        if (!isVisible) return false;
        if (e.key === 'ArrowDown') {
            selectedIndex = Math.min(selectedIndex + 1, filteredHistory.length - 1);
            return true;
        } else if (e.key === 'ArrowUp') {
            selectedIndex = Math.max(selectedIndex - 1, 0);
            return true;
        } else if (e.key === 'Enter') {
            if (filteredHistory[selectedIndex]) {
                onSelect(filteredHistory[selectedIndex]);
            }
            return true;
        } else if (e.key === 'Escape') {
            onClose();
            return true;
        }
        return false;
    }
</script>

{#if isVisible && filteredHistory.length > 0}
    <div 
        class="rg-history-popup" 
        style="left: {position.x}px; top: {position.y}px;"
    >
        {#each filteredHistory as command, index}
            <div 
                class="rg-history-item" 
                class:selected={index === selectedIndex}
                onclick={() => onSelect(command)}
            >
                {command}
            </div>
        {/each}
    </div>
{/if}

<style>
    .rg-history-popup {
        position: absolute;
        background: var(--rg-bg, #1e1e1e);
        border: 1px solid var(--rg-border, #333);
        border-radius: 4px;
        box-shadow: 0 4px 12px rgba(0,0,0,.5);
        z-index: 100;
        max-height: 200px;
        overflow-y: auto;
        width: 300px;
    }
    .rg-history-item {
        padding: 4px 8px;
        cursor: pointer;
    }
    .rg-history-item.selected {
        background: var(--rg-accent, #4a8cff);
        color: white;
    }
</style>
