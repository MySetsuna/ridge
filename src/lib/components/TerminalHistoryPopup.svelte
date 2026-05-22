<script lang="ts">
    import { terminalHistoryStore, terminalHistoryLoadedStore, dedupKeepFirst, filterByPrefix } from '$lib/stores/terminalHistory';
    interface Props {
        query: string;
        isVisible: boolean;
        /** Pick a history entry. `execute=true` (Enter) sends the command
         *  followed by '\r' so the shell executes it immediately;
         *  `execute=false` (ArrowRight, Warp-style) inserts the command
         *  into the current shell line WITHOUT '\r' so the user can edit
         *  before pressing Enter themselves. */
        onSelect: (command: string, execute: boolean) => void;
        onClose: () => void;
        position: { x: number; y: number; inputH: number };
    }

    let { query, isVisible, onSelect, onClose, position }: Props = $props();

    // §1.31 (2026-05-20): filter / dedup logic extracted to
    // `$lib/stores/terminalHistory` so it can be unit-tested in Vitest.
    // Store invariant: newest-first (see `terminalHistoryStore.add`
    // and the matching backend dedup in `commands/terminal.rs`), so
    // `dedupKeepFirst` keeps the most-recent occurrence of each command.
    let filteredHistory = $derived(filterByPrefix(dedupKeepFirst($terminalHistoryStore), query));
    let selectedIndex = $state(-1);
    let popupEl: HTMLDivElement | undefined = $state();
    let showAbove = $state(true);

    const POPUP_MAX_H = 260;
    const GAP = 6;

    // 每次唤起时重置选中行
    $effect(() => {
        if (isVisible) selectedIndex = -1;
    });

    // 匹配项消失时自动关闭弹层，避免再次匹配时自动出现。
    // 但必须满足两个前置条件：
    //   1. 历史已加载完成（terminalHistoryLoadedStore=true）—— 否则
    //      首次按 ArrowUp 时 fetch 还没回来，空数组会让弹窗一开就关。
    //   2. 用户敲过字（query 非空）—— 否则用户只想浏览全部历史时
    //      若 store 暂时为空也会被秒关。
    $effect(() => {
        if (isVisible && $terminalHistoryLoadedStore && query.length > 0 && filteredHistory.length === 0) {
            onClose();
        }
    });

    $effect(() => {
        if (!isVisible || filteredHistory.length === 0) return;
        const spaceAbove = position.y - GAP;
        const spaceBelow = window.innerHeight - position.y - GAP;
        showAbove = spaceAbove >= POPUP_MAX_H || spaceAbove >= spaceBelow;
    });

    function scrollSelectedIntoView() {
        requestAnimationFrame(() => {
            if (!popupEl) return;
            const selected = popupEl.querySelector('.rg-history-item.selected');
            selected?.scrollIntoView({ block: 'nearest' });
        });
    }

    export function handleKeyDown(e: KeyboardEvent) {
        if (!isVisible) return false;
        if (e.key === 'ArrowDown') {
            if (selectedIndex === -1) {
                selectedIndex = 0;
            } else if (selectedIndex >= filteredHistory.length - 1) {
                selectedIndex = -1;
            } else {
                selectedIndex = selectedIndex + 1;
            }
            scrollSelectedIntoView();
            return true;
        } else if (e.key === 'ArrowUp') {
            if (selectedIndex === -1) {
                selectedIndex = filteredHistory.length - 1;
            } else if (selectedIndex === 0) {
                selectedIndex = -1;
            } else {
                selectedIndex = selectedIndex - 1;
            }
            scrollSelectedIntoView();
            return true;
        } else if (e.key === 'Enter') {
            if (selectedIndex === -1) {
                onClose();
            } else if (filteredHistory[selectedIndex]) {
                onSelect(filteredHistory[selectedIndex], true);
            }
            return true;
        } else if (e.key === 'ArrowRight') {
            // §1.33 (2026-05-22) — Warp-style: insert the selected command
            // into the shell line WITHOUT '\r' so the user can edit
            // before executing. If no row is selected, fall through
            // (returning false) so ArrowRight still behaves as a normal
            // cursor-right keystroke in the underlying shell.
            if (selectedIndex >= 0 && filteredHistory[selectedIndex]) {
                onSelect(filteredHistory[selectedIndex], false);
                return true;
            }
            return false;
        } else if (e.key === 'Escape') {
            onClose();
            return true;
        }
        return false;
    }
</script>

<div 
    bind:this={popupEl}
    class="rg-history-popup"
    class:above={showAbove}
    class:below={!showAbove}
    class:rg-hidden={!isVisible}
    style="left: {position.x}px; top: {showAbove ? position.y : position.y + position.inputH}px;"
>
    <button type="button"
        class="rg-history-item rg-history-dismiss"
        class:selected={selectedIndex === -1}
        onclick={onClose}
    >..</button>
    <div class="rg-history-divider"></div>
    {#each filteredHistory as command, index}
        <button type="button"
            class="rg-history-item"
            class:selected={index === selectedIndex}
            title={command}
            onclick={() => onSelect(command, true)}
        >
            <!-- §1.31 (2026-05-20): collapse embedded newlines to a single
                 visual marker so multi-line history entries (heredocs,
                 explicit `\n` inside quotes) don't break the row layout.
                 The full original command is preserved in `title=` for
                 hover-reveal and is what gets sent back to onSelect. -->
            {command.replace(/[\r\n]+/g, ' ↵ ')}
        </button>
    {/each}
</div>

<style>
    .rg-history-popup {
        position: fixed;
        background: var(--rg-surface);
        border: 1px solid var(--rg-border);
        border-radius: 6px;
        box-shadow: 0 8px 24px rgba(0,0,0,.45);
        z-index: 2147483647;
        max-height: 260px;
        overflow-y: auto;
        width: max-content;
        min-width: 200px;
        max-width: min(80vw, 800px);
        backdrop-filter: blur(8px);
        scrollbar-width: thin;
        scrollbar-color: var(--rg-scrollbar) transparent;
    }
    .rg-history-popup.rg-hidden {
        display: none;
    }
    .rg-history-popup.above {
        transform: translateY(calc(-100% - 6px));
    }
    .rg-history-popup.below {
        transform: translateY(6px);
    }
    .rg-history-item {
        display: block;
        width: 100%;
        padding: 6px 12px;
        cursor: pointer;
        color: var(--rg-fg);
        background: transparent;
        border: none;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        font-family: 'JetBrains Mono', 'Cascadia Code', 'SF Mono', Consolas, ui-monospace, monospace;
        font-size: 13px;
        line-height: 1.5;
        text-align: left;
        transition: background 0.1s ease;
    }
    .rg-history-item + .rg-history-item {
        border-top: 1px solid var(--rg-border);
    }
    .rg-history-item:hover {
        background: color-mix(in srgb, var(--rg-accent) 10%, transparent);
    }
    .rg-history-item.selected {
        background: var(--rg-accent);
        color: var(--rg-bg);
    }
    .rg-history-item.selected:hover {
        background: var(--rg-accent);
    }
    .rg-history-divider {
        height: 1px;
        margin: 4px 8px;
        background: var(--rg-border);
    }
    .rg-history-dismiss {
        padding: 6px 12px;
        color: var(--rg-fg-muted);
        font-size: 13px;
        line-height: 1.5;
        text-align: left;
        font-family: 'JetBrains Mono', 'Cascadia Code', 'SF Mono', Consolas, ui-monospace, monospace;
    }
    .rg-history-dismiss.selected {
        background: var(--rg-accent);
        color: var(--rg-bg);
    }
</style>
