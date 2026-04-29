# @ridge/split

Lightweight split-pane primitives for Svelte 5.

Three components — `<RgSplit>` (flex container), `<RgPane>` (percentage-sized child) and `<RgSplitter>` (visual + clickable divider). All rendering is pure: there is no internal drag state machine. Consumers wire `mousedown` on `<RgSplitter>` and update the `size` prop on each `<RgPane>` themselves. This gives full freedom to layer snap / coupled resize / persistence on top.

## Quick start

```svelte
<script>
  import { RgSplit, RgPane, RgSplitter } from '@ridge/split';

  let leftSize = $state(30);
  let rightSize = $state(70);

  function onSplitterDown(e: MouseEvent) {
    /* your own drag impl: window mousemove → update leftSize / rightSize → mouseup unbind */
  }
</script>

<RgSplit direction="horizontal" class="h-full w-full">
  <RgPane size={leftSize}>left</RgPane>
  <RgSplitter onmousedown={onSplitterDown} />
  <RgPane size={rightSize}>right</RgPane>
</RgSplit>
```

## Why "pure rendering"

Because every non-trivial app needs custom drag semantics — junction snap, coupled multi-axis resize, undo, persistence. Bundling drag into the library means consumers fight the library to insert their own logic. We keep render and drag separate; the cost is one short `mousemove` handler in your codebase.

## Theming

Override these CSS variables on the splitter (or any ancestor):

| Variable | Default | Use |
|--|--|--|
| `--rg-splitter-color` | `rgba(255,255,255,.06)` | Idle line color |
| `--rg-splitter-active-color` | `#a78bfa` | Hover / drag color |
| `--rg-splitter-active-glow` | `rgba(167,139,250,.45)` | Hover / drag glow |

## License

MIT
