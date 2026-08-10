/**
 * autoGrow — textarea 随内容自动增高，封顶 maxRows 行（超出转滚动）。
 * 传入 `value` 使程序化改值（如发送后清空）也触发重算（action update 钩子）。
 */
export interface AutoGrowOpts {
  maxRows?: number;
  /** 绑定的当前值——仅用于触发 update 重算，行为不读取其内容。 */
  value?: string;
}

export function autoGrow(node: HTMLTextAreaElement, opts: AutoGrowOpts = {}) {
  let maxRows = opts.maxRows ?? 3;
  const fit = () => {
    node.style.height = 'auto';
    const cs = getComputedStyle(node);
    const line = Number.parseFloat(cs.lineHeight) || 16;
    const pad = (Number.parseFloat(cs.paddingTop) || 0) + (Number.parseFloat(cs.paddingBottom) || 0);
    const border = (Number.parseFloat(cs.borderTopWidth) || 0) + (Number.parseFloat(cs.borderBottomWidth) || 0);
    const max = Math.ceil(line * maxRows + pad + border);
    const want = node.scrollHeight + border;
    node.style.height = `${Math.min(want, max)}px`;
    node.style.overflowY = want > max ? 'auto' : 'hidden';
  };
  node.addEventListener('input', fit);
  fit();
  return {
    update(next: AutoGrowOpts = {}) {
      maxRows = next.maxRows ?? maxRows;
      fit();
    },
    destroy() {
      node.removeEventListener('input', fit);
    },
  };
}
