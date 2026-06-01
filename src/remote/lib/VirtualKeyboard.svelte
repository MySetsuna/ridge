<script lang="ts">
  interface ModState {
    ctrl: boolean;
    alt: boolean;
    shift: boolean;
  }

  let { onKey }: {
    onKey: (key: string, ctrl: boolean, alt: boolean, shift: boolean) => void;
  } = $props();

  let mods = $state<ModState>({ ctrl: false, alt: false, shift: false });

  function toggleMod(m: 'ctrl' | 'alt' | 'shift') {
    mods = { ...mods, [m]: !mods[m] };
  }

  function sendNamedKey(key: string) {
    onKey(key, mods.ctrl, mods.alt, mods.shift);
    mods = { ctrl: false, alt: false, shift: false };
  }

  function sendArrow(dir: string) {
    onKey('Arrow' + dir, mods.ctrl, mods.alt, mods.shift);
    mods = { ctrl: false, alt: false, shift: false };
  }

  $effect(() => {
    function handleGlobalKey(e: KeyboardEvent) {
      if (['Control', 'Alt', 'Shift'].includes(e.key)) return;
      if (mods.ctrl || mods.alt || mods.shift) {
        mods = { ctrl: false, alt: false, shift: false };
      }
    }
    window.addEventListener('keydown', handleGlobalKey);
    return () => window.removeEventListener('keydown', handleGlobalKey);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- preventDefault on pointerdown so tapping a key never blurs the terminal's
     hidden textarea — that would dismiss the mobile soft keyboard. -->
<div class="vk-container" onpointerdown={(e) => e.preventDefault()}>
  <div class="vk-row">
    <button
      class="vk-key mod"
      class:active={mods.ctrl}
      onclick={() => toggleMod('ctrl')}
    >Ctrl</button>
    <button
      class="vk-key mod"
      class:active={mods.alt}
      onclick={() => toggleMod('alt')}
    >Alt</button>
    <button
      class="vk-key mod"
      class:active={mods.shift}
      onclick={() => toggleMod('shift')}
    >Shift</button>
    <span class="vk-sep"></span>
    <button class="vk-key" onclick={() => sendNamedKey('Escape')}>Esc</button>
    <button class="vk-key" onclick={() => sendNamedKey('Tab')}>Tab</button>
    <button class="vk-key wide" onclick={() => sendNamedKey('Enter')}>⏎ Enter</button>
    <button class="vk-key" aria-label="Backspace" onclick={() => sendNamedKey('Backspace')}>⌫</button>
  </div>
  <div class="vk-row">
    <button class="vk-key arrow" onclick={() => sendArrow('Left')}>←</button>
    <button class="vk-key arrow" onclick={() => sendArrow('Down')}>↓</button>
    <button class="vk-key arrow" onclick={() => sendArrow('Up')}>↑</button>
    <button class="vk-key arrow" onclick={() => sendArrow('Right')}>→</button>
    <span class="vk-sep"></span>
    <button class="vk-key" onclick={() => sendNamedKey('Home')}>Home</button>
    <button class="vk-key" onclick={() => sendNamedKey('End')}>End</button>
  </div>
</div>

<style>
  .vk-container {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 8px;
    background: var(--rg-surface);
    border-top: 1px solid var(--rg-border-bright);
    border-bottom: 1px solid var(--rg-border-bright);
    user-select: none;
    -webkit-user-select: none;
    touch-action: manipulation;
  }
  .vk-row {
    display: flex;
    align-items: center;
    gap: 4px;
    justify-content: center;
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
  .vk-sep {
    width: 1px;
    height: 24px;
    background: var(--rg-border-bright);
    margin: 0 2px;
  }
</style>
