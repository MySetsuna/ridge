# rg-split

[![npm](https://img.shields.io/npm/v/rg-split)](https://www.npmjs.com/package/rg-split)
[![license](https://img.shields.io/npm/l/rg-split)](./LICENSE)

Lightweight split-pane primitives for **Svelte 5**.

Three components — `<RgSplit>` (flex container), `<RgPane>` (percentage-sized child), `<RgSplitter>` (visual divider). There is no internal drag state machine — consumers wire `mousedown` on `<RgSplitter>` and update `size` props on each `<RgPane>`. This gives full freedom to layer snap, coupled resize, or persistence on top.

**[→ Live examples](https://YOUR_USERNAME.github.io/rg-split/)**

## Install

```bash
npm install rg-split
# or
pnpm add rg-split
```

## Quick start

```svelte
<script lang="ts">
  import { RgSplit, RgPane, RgSplitter } from 'rg-split';

  let sizes = $state([30, 70]);
  let container: HTMLElement;

  function drag(e: MouseEvent) {
    const startX = e.clientX;
    const start = [...sizes];
    const move = (ev: MouseEvent) => {
      const rect = container.getBoundingClientRect();
      const d = ((ev.clientX - startX) / rect.width) * 100;
      const a = Math.max(10, Math.min(90, start[0] + d));
      sizes = [a, 100 - a];
    };
    addEventListener('mousemove', move);
    addEventListener('mouseup', () =>
      removeEventListener('mousemove', move), { once: true });
  }
</script>

<div bind:this={container} style="height: 400px">
  <RgSplit direction="horizontal" style="height: 100%">
    <RgPane size={sizes[0]}>Left</RgPane>
    <RgSplitter onmousedown={drag} />
    <RgPane size={sizes[1]}>Right</RgPane>
  </RgSplit>
</div>
```

## API

### `<RgSplit>`

| Prop | Type | Description |
|------|------|-------------|
| `direction` | `'horizontal' \| 'vertical'` | Layout axis |
| `class` | `string?` | Forwarded to root `<div>` |

### `<RgPane>`

| Prop | Type | Description |
|------|------|-------------|
| `size` | `number` | Percentage of parent main axis (0–100) |
| `class` | `string?` | Forwarded to pane `<div>` |

### `<RgSplitter>`

| Prop | Type | Description |
|------|------|-------------|
| `dragging` | `boolean?` | Applies active styles while dragging |
| `onmousedown` | `(e: MouseEvent) => void` | Consumer drag-start handler |
| `class` | `string?` | Forwarded to splitter `<div>` |

## Theming

Override CSS custom properties on the splitter or any ancestor:

| Variable | Default | Use |
|----------|---------|-----|
| `--rg-splitter-color` | `rgba(255,255,255,.06)` | Idle line color |
| `--rg-splitter-active-color` | `#a78bfa` | Hover / drag color |
| `--rg-splitter-active-glow` | `rgba(167,139,250,.45)` | Hover / drag glow |

## Why "pure rendering"

Every non-trivial app needs custom drag semantics — snap, coupled multi-axis resize, undo, persistence. Bundling drag into the library means consumers fight the library to insert their own logic. We keep render and drag separate.

## License

MIT
