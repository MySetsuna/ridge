<script lang="ts">
  // §2 修复：Ctrl/Alt/Shift 改用 modState 的共享 sticky 修饰键，使其能与**设备
  // 软键盘打出的普通字符**组成快捷键（旧实现的本地 mods 只作用于本栏命名键，
  // 软键盘字符走 TerminalCanvas.handleInput 读不到 → 组合键失效）。一次性语义：
  // 武装后下一个键（命名键或软键盘字符）消费即释放（见 modState consumeMods）。
  import { stickyMods, toggleMod, peekMods, clearMods } from './modState.svelte';

  let { onKey, onArm }: {
    onKey: (key: string, ctrl: boolean, alt: boolean, shift: boolean) => void;
    onArm?: () => void;
  } = $props();

  function tapMod(m: 'ctrl' | 'alt' | 'shift') {
    const wasOn = stickyMods[m];
    toggleMod(m);
    if (!wasOn) onArm?.();
  }

  function sendNamedKey(key: string) {
    const m = peekMods();
    onKey(key, m.ctrl, m.alt, m.shift);
    clearMods();
  }

  function sendArrow(dir: string) {
    const m = peekMods();
    onKey('Arrow' + dir, m.ctrl, m.alt, m.shift);
    clearMods();
  }

  function sendPage(dir: 'Up' | 'Down') {
    const m = peekMods();
    onKey('Page' + dir, m.ctrl, m.alt, m.shift);
    clearMods();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- preventDefault on pointerdown so tapping a key never blurs the terminal's
     hidden textarea — that would dismiss the mobile soft keyboard. -->
<div class="vk-container" onpointerdown={(e) => e.preventDefault()}>
  <!-- Top row: modifiers + escape/tab/enter/backspace -->
  <div class="vk-row vk-row-mods">
    <button
      class="vk-key mod"
      class:active={stickyMods.ctrl}
      onclick={() => tapMod('ctrl')}
    >Ctrl</button>
    <button
      class="vk-key mod"
      class:active={stickyMods.alt}
      onclick={() => tapMod('alt')}
    >Alt</button>
    <button
      class="vk-key mod"
      class:active={stickyMods.shift}
      onclick={() => tapMod('shift')}
    >Shift</button>
    <span class="vk-sep"></span>
    <button class="vk-key" onclick={() => sendNamedKey('Escape')}>Esc</button>
    <button class="vk-key" onclick={() => sendNamedKey('Tab')}>Tab</button>
    <button class="vk-key wide" onclick={() => sendNamedKey('Enter')}>⏎ Enter</button>
    <button class="vk-key" aria-label="Backspace" onclick={() => sendNamedKey('Backspace')}>⌫</button>
  </div>

  <!-- Middle row: navigation cluster (arrow keys + home/end + pgup/pgdn) -->
  <div class="vk-row vk-row-nav">
    <div class="vk-nav-cluster">
      <button class="vk-key arrow" onclick={() => sendArrow('Up')} aria-label="Up">↑</button>
      <div class="vk-arrow-row">
        <button class="vk-key arrow" onclick={() => sendArrow('Left')} aria-label="Left">←</button>
        <button class="vk-key arrow" onclick={() => sendArrow('Down')} aria-label="Down">↓</button>
        <button class="vk-key arrow" onclick={() => sendArrow('Right')} aria-label="Right">→</button>
      </div>
    </div>
    <span class="vk-sep"></span>
    <div class="vk-page-cluster">
      <button class="vk-key home" onclick={() => sendNamedKey('Home')} aria-label="Home">Home</button>
      <button class="vk-key end" onclick={() => sendNamedKey('End')} aria-label="End">End</button>
      <button class="vk-key pgup" onclick={() => sendPage('Up')} aria-label="Page Up">PgUp</button>
      <button class="vk-key pgdn" onclick={() => sendPage('Down')} aria-label="Page Down">PgDn</button>
    </div>
  </div>
</div>

<style>
  .vk-container {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 8px;
    background: var(--rg-surface);
    user-select: none;
    -webkit-user-select: none;
    touch-action: manipulation;
    transition: transform .2s ease;
  }
  .vk-row {
    display: flex;
    align-items: center;
    gap: 4px;
    justify-content: center;
  }
  .vk-row-mods {
    flex-wrap: wrap;
  }
  .vk-row-nav {
    justify-content: space-between;
    gap: 8px;
    padding-top: 2px;
  }
  .vk-nav-cluster {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }
  .vk-arrow-row {
    display: flex;
    gap: 2px;
  }
  .vk-page-cluster {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .vk-key {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 44px;
    height: 36px;
    padding: 0 10px;
    border: 1px solid var(--rg-border-bright);
    border-radius: 8px;
    background: var(--rg-bg);
    color: var(--rg-fg);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.12s;
    touch-action: manipulation;
    -webkit-tap-highlight-color: transparent;
  }
  .vk-key:active {
    background: var(--rg-surface-2);
    transform: scale(.95);
  }
  .vk-key.mod.active {
    background: color-mix(in srgb, var(--rg-accent) 25%, transparent);
    border-color: var(--rg-accent);
    color: var(--rg-accent);
  }
  .vk-key.arrow {
    min-width: 48px;
    font-size: 16px;
  }
  .vk-key.wide {
    min-width: 66px;
  }
  .vk-key.home,
  .vk-key.end,
  .vk-key.pgup,
  .vk-key.pgdn {
    min-width: 52px;
    font-size: 10px;
    font-weight: 600;
    padding: 0 6px;
  }
  .vk-sep {
    width: 1px;
    height: 24px;
    background: var(--rg-border-bright);
    margin: 0 2px;
  }
</style>
