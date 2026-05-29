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

  // Clear sticky modifiers once a physical key fires elsewhere.
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

<!-- Compact, always-visible quick-key strip. Horizontally scrollable so it
     never wraps; sticks above the OS soft keyboard (see MainApp viewport sync). -->
<div class="vk">
  <button class="vk-key mod" class:active={mods.ctrl} onclick={() => toggleMod('ctrl')}>Ctrl</button>
  <button class="vk-key mod" class:active={mods.alt} onclick={() => toggleMod('alt')}>Alt</button>
  <button class="vk-key mod" class:active={mods.shift} onclick={() => toggleMod('shift')}>Sft</button>
  <span class="vk-sep"></span>
  <button class="vk-key" onclick={() => sendNamedKey('Escape')}>Esc</button>
  <button class="vk-key" onclick={() => sendNamedKey('Tab')}>Tab</button>
  <button class="vk-key wide" onclick={() => sendNamedKey('Enter')}>↵</button>
  <span class="vk-sep"></span>
  <button class="vk-key" onclick={() => sendNamedKey('ArrowUp')}>↑</button>
  <button class="vk-key" onclick={() => sendNamedKey('ArrowDown')}>↓</button>
  <button class="vk-key" onclick={() => sendNamedKey('ArrowLeft')}>←</button>
  <button class="vk-key" onclick={() => sendNamedKey('ArrowRight')}>→</button>
  <span class="vk-sep"></span>
  <button class="vk-key" onclick={() => sendNamedKey('PageUp')}>PgUp</button>
  <button class="vk-key" onclick={() => sendNamedKey('PageDown')}>PgDn</button>
</div>

<style>
  .vk {
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 4px 6px;
    background: #161b22;
    border-top: 1px solid #30363d;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
    -webkit-overflow-scrolling: touch;
    user-select: none;
    -webkit-user-select: none;
    touch-action: manipulation;
    flex-shrink: 0;
  }
  .vk::-webkit-scrollbar { display: none; }
  .vk-key {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 38px;
    height: 32px;
    padding: 0 8px;
    border: 1px solid #30363d;
    border-radius: 7px;
    background: #0d1117;
    color: #e6edf3;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    flex-shrink: 0;
    transition: background 0.1s, transform 0.1s;
    touch-action: manipulation;
    -webkit-tap-highlight-color: transparent;
  }
  .vk-key.wide { min-width: 46px; font-size: 15px; }
  .vk-key:active { background: #30363d; transform: scale(0.94); }
  .vk-key.mod.active {
    background: rgba(88,166,255,.25);
    border-color: #58a6ff;
    color: #58a6ff;
  }
  .vk-sep { width: 1px; height: 20px; background: #30363d; flex-shrink: 0; margin: 0 1px; }
</style>
