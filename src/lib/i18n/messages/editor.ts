/** editor 命名空间：文件编辑器 / Markdown 预览。 */
export const zh: Record<string, string> = {
  // 工具栏
  toolbarAriaLabel: '编辑器工具栏',
  collapsePanel: '收起编辑器面板',
  unsaved: '● 未保存',
  saved: '已保存',
  // Diff 控件
  diffSideBySide: '并排 diff',
  diffInline: '内联 diff',
  diffReload: '重新加载 diff',
  // 工具按钮
  find: '查找 (Ctrl+F)',
  save: '保存 (Ctrl+S)',
  settings: '设置',
  // 设置下拉
  displayMode: '显示模式',
  modeEmbedded: '嵌入模式',
  modeDrawer: '抽屉模式',
  modeFloating: '悬浮 Pin 模式',
  modeWindow: '独立窗口',
  revertFile: '放弃修改（重新从磁盘加载）',
  closeAllTabs: '关闭全部标签',
  hidePanel: '隐藏编辑器面板',
  // 浮动窗口
  closeFloating: '关闭浮动窗口（保留打开的文件）',
  // Tab 标签
  externalDeleted: '{name} 已被外部删除',
  deletedBadge: '已删除',
  externalDeletedTitle: '文件已被外部删除',
  unsavedDot: '未保存',
  closeTab: '关闭',
  // Diff 加载
  diffLoading: '加载中…',
  // Markdown 预览切换
  switchToSource: '切换到源码编辑 (Markdown)',
  switchToPreview: '切换到预览 (Markdown)',
  sourceLabel: '源码',
  previewLabel: '预览',
  // 拖拽调整大小
  resizeEditorWidth: '调整编辑器宽度',
  resizeTop: '从上边调整',
  resizeBottom: '从下边调整',
  resizeRight: '从右边调整',
  resizeLeft: '从左边调整',
  resizeNE: '右上',
  resizeNW: '左上',
  resizeSE: '右下',
  resizeSW: '左下',
  // 错误
  requiresTauri: '需要 Tauri 环境',
  // 右键菜单
  ctxClose: '关闭',
  ctxCloseOthers: '关闭其他',
  ctxCloseRight: '关闭右侧',
  ctxCloseSaved: '关闭已保存',
  ctxCloseAll: '关闭全部',
  ctxCopyPath: '复制路径',
  ctxCopyName: '复制文件名',
  ctxReveal: '在文件资源管理器中显示',
  ctxCopyFailed: '复制失败',
  ctxOpenFailed: '打开失败',
};

export const en: Record<string, string> = {
  // Toolbar
  toolbarAriaLabel: 'Editor toolbar',
  collapsePanel: 'Collapse editor panel',
  unsaved: '● Unsaved',
  saved: 'Saved',
  // Diff controls
  diffSideBySide: 'Side-by-side diff',
  diffInline: 'Inline diff',
  diffReload: 'Reload diff',
  // Tool buttons
  find: 'Find (Ctrl+F)',
  save: 'Save (Ctrl+S)',
  settings: 'Settings',
  // Settings dropdown
  displayMode: 'Display mode',
  modeEmbedded: 'Embedded',
  modeDrawer: 'Drawer',
  modeFloating: 'Floating (Pin)',
  modeWindow: 'Independent window',
  revertFile: 'Revert (reload from disk)',
  closeAllTabs: 'Close all tabs',
  hidePanel: 'Hide editor panel',
  // Floating window
  closeFloating: 'Close floating window (keep open files)',
  // Tab labels
  externalDeleted: '{name} deleted externally',
  deletedBadge: 'Deleted',
  externalDeletedTitle: 'File was deleted externally',
  unsavedDot: 'Unsaved',
  closeTab: 'Close',
  // Diff loading
  diffLoading: 'Loading…',
  // Markdown preview toggle
  switchToSource: 'Switch to source (Markdown)',
  switchToPreview: 'Switch to preview (Markdown)',
  sourceLabel: 'Source',
  previewLabel: 'Preview',
  // Resize handles
  resizeEditorWidth: 'Resize editor width',
  resizeTop: 'Resize from top',
  resizeBottom: 'Resize from bottom',
  resizeRight: 'Resize from right',
  resizeLeft: 'Resize from left',
  resizeNE: 'Top-right corner',
  resizeNW: 'Top-left corner',
  resizeSE: 'Bottom-right corner',
  resizeSW: 'Bottom-left corner',
  // Errors
  requiresTauri: 'Requires Tauri environment',
  // Context menu
  ctxClose: 'Close',
  ctxCloseOthers: 'Close others',
  ctxCloseRight: 'Close to the right',
  ctxCloseSaved: 'Close saved',
  ctxCloseAll: 'Close all',
  ctxCopyPath: 'Copy path',
  ctxCopyName: 'Copy filename',
  ctxReveal: 'Reveal in file explorer',
  ctxCopyFailed: 'Copy failed',
  ctxOpenFailed: 'Open failed',
};
