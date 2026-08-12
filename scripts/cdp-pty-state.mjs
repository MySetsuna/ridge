export function shouldRequestPaneList(summary, state) {
  return !summary.pane && !state.createRequested;
}
