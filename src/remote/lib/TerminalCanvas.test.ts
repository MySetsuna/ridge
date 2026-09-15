import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./TerminalCanvas.svelte', import.meta.url), 'utf8');
const lines = source.split(/\r?\n/);

function lineAfter(marker: string, offset = 0): number {
	const idx = lines.findIndex((l) => l.includes(marker));
	if (idx < 0) throw new Error(`marker not found: ${marker}`);
	return idx + 1 + offset;
}

function withinRange(startMarker: string, endMarker: string, body: string): boolean {
	const start = lines.findIndex((l) => l.includes(startMarker));
	if (start < 0) return false;
	const end = lines.findIndex((l, i) => i > start && l.includes(endMarker));
	if (end < 0) return false;
	const range = lines.slice(start, end).join('\n');
	return range.includes(body);
}

describe('remote pane Agent status chrome contract', () => {
  it('surfaces actionable WebGPU initialization failures', () => {
    expect(source).toContain('let attachError = $state<string | null>(null);');
    expect(source).toContain('hostError?: string | null;');
    expect(source).toContain('role="alert"');
    expect(source).toContain('WEBGPU_INIT_FAILED');
    expect(source).toContain("backendName = $bindable('WebGPU')");
  });

  it('uses only a transient intervention rail without changing terminal geometry', () => {
    expect(source).toContain('class:agent-needs-attention={agentNeedsAttention}');
    expect(source).toContain('.container.agent-needs-attention{box-shadow:inset 0 0 0 2px var(--rg-ansi-yellow,#d29922)}');
    expect(source).not.toContain('.container.agent-working{');
    expect(source).not.toContain('.container.agent-starting{');
    expect(source).not.toContain('.container.agent-idle{');
  });

  it('clears the transient rail when the pane input surface receives focus', () => {
    expect(source).toContain('onFocus: onPaneFocus');
    expect(source).toContain('onPaneFocus?.(pane)');
    expect(source).toContain('manager.setFocused(paneId, true)');
  });

  it('keeps switch-gap frames and first keystrokes instead of dropping them', () => {
    expect(source).toContain('onDrainPending');
    expect(source).toContain('const pendingFrames = onDrainPending?.(paneId) ?? []');
    expect(source).toContain('const MAX_PENDING_STDIN_BYTES = 64 * 1024;');
    expect(source).toContain('focusInput();');
    expect(source).toMatch(/if \(!attached\) \{\r?\n\s+onStdin\(text\);/);
  });

  it('keeps renderer cursor ownership when the mobile IME sink blurs', () => {
    expect(source).toContain('manager.setFocused(paneId, true);');
    expect(source).not.toContain('onblur={() => manager.setFocused(paneId, false)}');
    expect(source).toContain('caret-color:transparent');
    expect(source).not.toContain('caret-color:var(--rg-accent,#58a6ff)');
  });

  it('pins IME composition to the captured input cursor and updates after PTY input', () => {
    expect(source).toContain('manager.beginImeComposition(paneId);');
    expect(source).toContain('manager.endImeComposition(paneId);');
    expect(source).toContain('manager.noteUserInput(paneId);');
    expect(source).toContain('manager.onImeAnchor(paneId');
    expect(source).toContain('pinImeCaretToAnchor(el);');
    expect(source).toMatch(/positionInputAtCursorOrCenter\(\);\r?\n\s+if \(sbufActive\(\)/);
  });

  it('keeps pane geometry authoritative for host resize recovery', () => {
    expect(source).toContain('export function fitPaneNow()');
    expect(source).toContain('if (attached) manager.fitPaneNow(paneId);');
    expect(source).toContain('await manager.fitPaneNow(paneId, true);');
    expect(source.indexOf('await manager.fitPaneNow(paneId, true);')).toBeLessThan(
      source.indexOf('const pendingFrames = onDrainPending?.(paneId) ?? [];'),
    );
    expect(source).toContain('return onPaneResize(pane, rows, cols');
    expect(source).toContain('export function claimPaneSize(): Promise<void>');
    expect(source).toContain('for (let frame = 0; frame < 30; frame += 1)');
    expect(source).toContain('const viewport = window.visualViewport;');
    expect(source).toContain('manager.resizeHost();');
    expect(source).toContain('await manager.claimPaneSize(paneId);');
    expect(source).toContain('manager.forceFullRedraw(paneId)');
    expect(source).toContain('export function resizeKernel(rows: number, cols: number)');
    expect(source).toContain('manager.applyPaneResize(paneId, rows, cols)');
    expect(source).toContain('manager.forceFullRedraw(paneId)');
  });

  it('does not promote the browser viewer to local grid authority', () => {
    expect(source).not.toContain('manager.setLocalGridAuthority(paneId, true);');
  });

  it('does not drop mobile spaces reported as insertCompositionText', () => {
    expect(source).toContain('Some mobile keyboards');
    expect(source).toContain('if (text === imeCommitExpect && Date.now() - imeCommitExpectTime < IME_DUP_WINDOW_MS)');
    expect(source).not.toContain("if (inputType === 'insertCompositionText') return;");
  });

  it('forwards touch press-drag-release to mouse-reporting TUIs', () => {
    expect(source).toContain('let touchMouseDragging = false;');
    expect(source).toContain("decideTouchMouseGesture('press')");
    expect(source).toContain("decideTouchMouseGesture('drag')");
    expect(source).toContain("decideTouchMouseGesture('release')");
    expect(source).toContain('manager.hasLinkAt(paneId, startCell.row, startCell.col)');
    expect(source).toContain('ontouchcancel={handleTouchCancel}');
  });

  it('keeps explicit selection local even when a TUI captures the mouse', () => {
    expect(source).toContain('Explicit selection mode always belongs to the controller.');
    expect(source).toContain('if (cell) startSelection(cell.row, cell.col);');
    expect(source).toContain('if (cell) extendSelection(cell.row, cell.col);');
    expect(source).not.toContain('§select-as-mouse');
  });

  it('cancels compatibility clicks before focusing the mobile IME sink', () => {
    expect(source).toContain('Cancel its compatibility');
    expect(source).toMatch(/if \(elapsed >= TOUCH_TAP_MAX_MS\) return;\r?\n\s+\/\/ Focus happens[\s\S]*?e\.preventDefault\(\);/);
    expect(source).toMatch(/if \(!wasDragging\) \{\r?\n\s+e\.preventDefault\(\);\r?\n\s+openSoftKeyboard\(\);/);
  });

  it('seeds first-pane keyboard avoidance from the trusted tap', () => {
    expect(source).toMatch(/scrollToBottom: \(\) => \{[\s\S]*?manager\.scrollToBottom\(paneId\);[\s\S]*?manager\.captureImeAnchor\(paneId\);/);
    expect(source).toMatch(/activateIme\(\{[\s\S]*?requestAnimationFrame\(\(\) => \{[\s\S]*?requestKeyboardShift\(\);/);
    expect(source).toMatch(/manager\.onImeAnchor\(paneId,[\s\S]*?positionInputAtCursorOrCenter\(\);[\s\S]*?requestKeyboardShift\(\);/);
  });

  it('requires the platform modifier for mouse links while preserving touch taps', () => {
    expect(source).toContain('e.button === 0 && linkModifierHeld(e) && manager.openLinkAt(paneId, cell.row, cell.col)');
    expect(source).toContain('return isMac ? e.metaKey : e.ctrlKey;');
    expect(source).toContain('const linkCell = touchLinkCell;');
    expect(source).toMatch(/e\.preventDefault\(\);\r?\n\s+return;/);
  });

  it('reports the first post-switch frame without retaining payloads', () => {
    expect(source).toContain('onFirstPaint?: (paneKey: string) => void;');
    expect(source).toContain('requestAnimationFrame(() => {');
    expect(source).toContain('onFirstPaint?.(paneId);');
  });
});

// §D (Goal #2): 默认模式 swipe 始终是滚动；显式选择/鼠标模式按预期分派；
// 一次点击只派发一次；切终端后不残留按下状态；不机械改 touch-action。
// 这些断言覆盖 TerminalCanvas 实际 start/move/end/cancel 路径，不依赖
// selectionMode → local_scroll 的辅助函数。组件级运行时验证（直接调用
// handleTouch*）需要 @testing-library/svelte；当前测试栈没装，使用源码
// 路径断言 + 状态机不变量（cancel 必须清空所有 touch 状态，start → end
// → start 之间不残留）作为可执行证据。
describe('D — touch/mouse mode contract in TerminalCanvas.svelte source', () => {
  it('默认模式（无 mouse reporting）touchstart 不派发 mouse press', () => {
    // 关键不变量：handleTouchStart 里，selectionMode 关闭 + isMouseReporting()=false
    // → 不会走 `decideTouchMouseGesture('press')` / `kEncodeMouse` 路径。
    // 这里反向验证：handleTouchStart 中 mouse press 路径在 isMouseReporting()
    // 真值分支内。
    const startFnLine = lineAfter('function handleTouchStart', 0);
    const startBody = lines.slice(startFnLine, startFnLine + 40).join('\n');
    expect(startBody).toMatch(/if \(selectionMode\)/);
    expect(startBody).toMatch(/} else if \(isMouseReporting\(\) && !touchLinkCell\)/);
    // 任何无条件的 mouse press 派发都是 bug。
    expect(startBody).not.toMatch(/kEncodeMouse\([^)]*\)\s*;\s*onStdin/);
  });

  it('mouse reporting 开启且不在 link 上 → 派发 mouse press', () => {
    const startBody = lines
	  .slice(lineAfter('function handleTouchStart'), lineAfter('function handleTouchStart') + 40)
	  .join('\n');
    expect(startBody).toContain("decideTouchMouseGesture('press')");
    expect(startBody).toContain('touchMouseDragging = true');
  });

  it('mouse reporting 开启 + 命中 link cell → 走 link 预留路径，不发 mouse press', () => {
    // touchLinkCell 已被设为非空时，else if 条件 `!touchLinkCell` 失败 → 跳过 mouse press。
    const startBody = lines
	  .slice(lineAfter('function handleTouchStart'), lineAfter('function handleTouchStart') + 40)
	  .join('\n');
    expect(startBody).toMatch(/touchLinkCell\s*=\s*!\s*selectionMode\s*&&\s*startCell\s*&&\s*manager\.hasLinkAt/);
    expect(startBody).toMatch(/isMouseReporting\(\)\s*&&\s*!touchLinkCell/);
  });

  it('显式 selectionMode → touchstart 调 startSelection（不走 mouse 也不走 scroll）', () => {
    const startBody = lines
	  .slice(lineAfter('function handleTouchStart'), lineAfter('function handleTouchStart') + 40)
	  .join('\n');
    expect(startBody).toMatch(/if \(selectionMode\) \{[\s\S]*?startSelection\(cell\.row, cell\.col\);[\s\S]*?\}/);
  });

  it('touchmove 阈值以下不派发（不论哪种模式）', () => {
    const moveBody = lines
	  .slice(lineAfter('function handleTouchMove'), lineAfter('function handleTouchMove') + 25)
	  .join('\n');
    expect(moveBody).toContain('if (moved < TOUCH_DRAG_THRESHOLD_PX) return;');
  });

  it('默认模式（mouse reporting off）touchmove 超阈值 → 滚轮 + scrollUp/scrollDown，**不**发 mouse drag', () => {
    const moveBody = lines
	  .slice(lineAfter('function handleTouchMove'), lineAfter('function handleTouchMove') + 50)
	  .join('\n');
    // 滚动分支：scrollUp / scrollDown 通过 touchWheel 路径
    expect(moveBody).toMatch(/touchScrollAccum\s*\+=\s*touchLastY\s*-\s*t\.clientY/);
    expect(moveBody).toContain('touchWheel(touchScrollAccum');
    // mouse drag 分支只在 touchMouseDragging=true 时进入；验证条件互斥
    expect(moveBody).toMatch(/if \(touchMouseDragging\)/);
  });

  it('mouse reporting 模式 + touchMouseDragging → touchmove 派发 mouse drag', () => {
    const moveBody = lines
	  .slice(lineAfter('function handleTouchMove'), lineAfter('function handleTouchMove') + 50)
	  .join('\n');
    expect(moveBody).toContain("decideTouchMouseGesture('drag')");
  });

  it('selectionMode + 超阈值 → extendSelection（不发 mouse drag 也不发 scroll）', () => {
    const moveBody = lines
	  .slice(lineAfter('function handleTouchMove'), lineAfter('function handleTouchMove') + 50)
	  .join('\n');
    expect(moveBody).toMatch(/if \(selectionMode\) \{[\s\S]*?extendSelection\(cell\.row, cell\.col\);[\s\S]*?return;/);
  });

  it('touchend 派发 mouse release 严格在 touchMouseDragging 分支里', () => {
    const endBody = lines
	  .slice(lineAfter('function handleTouchEnd'), lineAfter('function handleTouchEnd') + 60)
	  .join('\n');
    expect(endBody).toMatch(/if \(touchMouseDragging\)\s*\{[\s\S]*?decideTouchMouseGesture\('release'\)[\s\S]*?touchMouseDragging\s*=\s*false/);
  });

  it('一次点击只派发一次 mouse：touchend 不重复发 mouse press', () => {
    // Goal: 一次点击只派发一次。touchend 走 openSoftKeyboard/clearSelection
    // 路径，**不**再次调 kEncodeMouse（已在 press 阶段发过）。
    const endBody = lines
	  .slice(lineAfter('function handleTouchEnd'), lineAfter('function handleTouchEnd') + 60)
	  .join('\n');
    // touchend 体内调 kEncodeMouse 必须在 touchMouseDragging 分支内
    const encodeMouseHits = (endBody.match(/kEncodeMouse\(/g) || []).length;
    // 至少 1 次（release 路径），但**不能**有非分支内的第二次
    expect(encodeMouseHits).toBeGreaterThanOrEqual(1);
    // 验证：kEncodeMouse 出现在 touchMouseDragging 块里
    const releaseBlock = endBody.match(/if \(touchMouseDragging\) \{[\s\S]*?touchMouseDragging = false/);
    expect(releaseBlock).not.toBeNull();
    expect(releaseBlock![0]).toContain('kEncodeMouse');
  });

  it('touchend 显式 selectionMode tap → 清选择 + openSoftKeyboard（不发 mouse）', () => {
    const endBody = lines
	  .slice(lineAfter('function handleTouchEnd'), lineAfter('function handleTouchEnd') + 60)
	  .join('\n');
    expect(endBody).toMatch(/if \(selectionMode\) \{[\s\S]*?manager\.clearSelection\(paneId\);[\s\S]*?openSoftKeyboard\(\);/);
  });

  it('touchend 默认模式 tap → openSoftKeyboard（无 mouse press，无 selection clear）', () => {
    const endBody = lines
	  .slice(lineAfter('function handleTouchEnd'), lineAfter('function handleTouchEnd') + 70)
	  .join('\n');
    // 默认模式 + 短 tap 分支：openSoftKeyboard
    expect(endBody).toMatch(/if \(elapsed >= TOUCH_TAP_MAX_MS\) return;[\s\S]*?openSoftKeyboard\(\);/);
  });

  it('touchcancel 必须清空所有 touch 状态（selDragging / touchMouseDragging / touchScrollAccum / touchLinkCell）', () => {
    // 切终端 / 系统手势中断 / 通知中心下拉 等情况：下一次 touchstart 不能残留
    // 任何「上一次未完成」状态，否则会双派发或派发到错的 PTY。
    const cancelBody = lines
	  .slice(lineAfter('function handleTouchCancel'), lineAfter('function handleTouchCancel') + 25)
	  .join('\n');
    expect(cancelBody).toContain('touchMouseDragging = false');
    expect(cancelBody).toMatch(/selDragging\s*=\s*false|isSelectingLocal\s*=\s*false/);
    expect(cancelBody).toContain('touchScrollAccum = 0');
    expect(cancelBody).toContain('touchLinkCell = null');
  });

  it('select-tap-keyboard：selectionMode 短 tap 也 raise 软键盘（不改变既有 openSoftKeyboard 入口）', () => {
    const endBody = lines
	  .slice(lineAfter('function handleTouchEnd'), lineAfter('function handleTouchEnd') + 60)
	  .join('\n');
    expect(endBody).toContain('// §select-tap-keyboard');
    expect(endBody).toMatch(/openSoftKeyboard\(\);/);
    // 验证没有把 openSoftKeyboard 改成别的入口
    expect(endBody).not.toMatch(/openVirtualKeyboard|focus\(\)|window\.focus/);
  });

  it('touchmove 不会无脑改 touch-action（不动 css）', () => {
    // 防止「机械修改 touch-action」反模式：css touch-action 应在 <style> 块
    // 而非在 touchmove handler 里 mutate。
    const moveBody = lines
	  .slice(lineAfter('function handleTouchMove'), lineAfter('function handleTouchMove') + 50)
	  .join('\n');
    expect(moveBody).not.toMatch(/container\.style\.touchAction|el\.style\.touchAction|touch-action\s*=/);
  });

  it('decideTouchMouseGesture 名字和 action 字符串契约（press/drag/release）', () => {
    // 防止有人改 decideTouchMouseGesture 名字或三态 action 字符串而不同步调用方
    expect(source).toMatch(/decideTouchMouseGesture\('press'\)/);
    expect(source).toMatch(/decideTouchMouseGesture\('drag'\)/);
    expect(source).toMatch(/decideTouchMouseGesture\('release'\)/);
  });
});
