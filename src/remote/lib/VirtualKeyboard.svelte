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
<!--
  §compact-layout v2: 紧凑双行，键位还原实体键盘以利盲打肌肉记忆——
  左下角修饰键(Ctrl/Alt/Shift) + 左上 Esc/Tab；中间方向键正-T(第一排 ←↑→，
  第二排仅 ↓，用 grid 让 ↑/↓ 同列纵向对齐)；右侧导航块 Home/PgUp 上、End/PgDn 下
  竖向配对；最右 Enter 上、Backspace 下。
  §no-overflow: 四组按列权重 flex 分摊整行宽度(键 flex:1; min-width:0)，键间/组间
  仅 2px——退格/回车恒落在屏内，绝不溢出窄屏右缘（旧版固定 min-width 之和 > 视口）。
-->
<div class="vk-container" onpointerdown={(e) => e.preventDefault()}>
  <!-- 左：Esc/Tab（上）+ Ctrl/Alt/Shift（下，实体键盘左下角） -->
  <div class="vk-group vk-left">
    <div class="vk-grp-row">
      <button class="vk-key" onclick={() => sendNamedKey('Escape')}>Esc</button>
      <button class="vk-key" onclick={() => sendNamedKey('Tab')}>Tab</button>
    </div>
    <div class="vk-grp-row">
      <button class="vk-key mod" class:active={stickyMods.ctrl} onclick={() => tapMod('ctrl')}>Ctrl</button>
      <button class="vk-key mod" class:active={stickyMods.alt} onclick={() => tapMod('alt')}>Alt</button>
      <button class="vk-key mod" class:active={stickyMods.shift} onclick={() => tapMod('shift')}>⇧</button>
    </div>
  </div>

  <!-- 中：方向键正-T（第一排 ←↑→，第二排仅 ↓，↑/↓ 同列对齐） -->
  <div class="vk-group vk-arrows">
    <button class="vk-key arrow a-left" onclick={() => sendArrow('Left')} aria-label="Left">←</button>
    <button class="vk-key arrow a-up" onclick={() => sendArrow('Up')} aria-label="Up">↑</button>
    <button class="vk-key arrow a-right" onclick={() => sendArrow('Right')} aria-label="Right">→</button>
    <button class="vk-key arrow a-down" onclick={() => sendArrow('Down')} aria-label="Down">↓</button>
  </div>

  <!-- 右中：导航块 Home/PgUp（上）/ End/PgDn（下） -->
  <div class="vk-group vk-nav">
    <button class="vk-key nav" onclick={() => sendNamedKey('Home')} aria-label="Home">Home</button>
    <button class="vk-key nav" onclick={() => sendPage('Up')} aria-label="Page Up">PgUp</button>
    <button class="vk-key nav" onclick={() => sendNamedKey('End')} aria-label="End">End</button>
    <button class="vk-key nav" onclick={() => sendPage('Down')} aria-label="Page Down">PgDn</button>
  </div>

  <!-- 右：Enter（上）/ Backspace（下） -->
  <div class="vk-group vk-right">
    <button class="vk-key wide" onclick={() => sendNamedKey('Enter')} aria-label="Enter">⏎</button>
    <button class="vk-key wide" aria-label="Backspace" onclick={() => sendNamedKey('Backspace')}>⌫</button>
  </div>
</div>

<style>
  /* §no-overflow: 一行四组(左修饰/中方向/右导航/最右Enter·⌫)，每组内部两排。
     四组按"列数"权重 flex 分摊 100% 行宽，键 flex:1+min-width:0 → 永不溢出窄屏。
     整体两排键高 ≈ 76px(含安全区前的内边距)。 */
  .vk-container {
    display: flex;
    align-items: stretch;
    gap: 2px;
    padding: 4px 4px calc(4px + env(safe-area-inset-bottom, 0px));
    width: 100%;
    box-sizing: border-box;
    background: var(--rg-surface);
    user-select: none;
    -webkit-user-select: none;
    touch-action: manipulation;
    transition: transform .2s ease;
  }
  .vk-group {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  /* 列权重：左修饰 3 / 方向 3 / 导航 2 / Enter·⌫ 1.5 —— 与各组最宽一排的列数对齐。 */
  .vk-left { flex: 3 1 0; }
  .vk-arrows { flex: 3 1 0; }
  .vk-nav { flex: 2 1 0; }
  .vk-right { flex: 1.5 1 0; }
  .vk-grp-row {
    display: flex;
    gap: 2px;
    min-width: 0;
  }
  .vk-grp-row .vk-key { flex: 1 1 0; }
  /* 方向键正-T：3 列 × 2 行网格。←↑→ 在第 1 排，↓ 在第 2 排第 2 列 → ↑/↓ 同列对齐。 */
  .vk-arrows {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: repeat(2, 1fr);
    gap: 2px;
  }
  .vk-arrows .a-left  { grid-column: 1; grid-row: 1; }
  .vk-arrows .a-up    { grid-column: 2; grid-row: 1; }
  .vk-arrows .a-right { grid-column: 3; grid-row: 1; }
  .vk-arrows .a-down  { grid-column: 2; grid-row: 2; }
  /* 导航块：2×2，Home/PgUp 上，End/PgDn 下。 */
  .vk-nav {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 2px;
  }
  .vk-key {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 0;
    height: 32px;
    padding: 0 2px;
    border: 1px solid var(--rg-border-bright);
    border-radius: 7px;
    background: var(--rg-bg);
    color: var(--rg-fg);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background .12s, transform .12s, border-color .12s, color .12s;
    touch-action: manipulation;
    -webkit-tap-highlight-color: transparent;
    overflow: hidden;
    white-space: nowrap;
  }
  .vk-key:active {
    background: var(--rg-surface-2);
    transform: scale(.94);
  }
  .vk-key.mod {
    flex: 1 1 0;
  }
  .vk-key.mod.active {
    background: color-mix(in srgb, var(--rg-accent) 25%, transparent);
    border-color: var(--rg-accent);
    color: var(--rg-accent);
  }
  .vk-key.arrow {
    font-size: 15px;
  }
  /* 导航键文字短，缩小字号让 2×2 块保持紧凑。 */
  .vk-key.nav {
    font-size: 10px;
    font-weight: 600;
  }
  /* Enter / Backspace 竖向占满该组高度。 */
  .vk-key.wide {
    flex: 1 1 0;
    font-size: 15px;
  }
</style>
