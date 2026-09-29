// §perf Monaco workers: web-remote build 映射全部 label 到轻量 editor.worker，
// 避免 7 MB ts.worker + html/css/json worker 进入 dist。桌面 Tauri build 不变。
//
// FileEditor.svelte module script 调用 setupMonacoWorkers()。

import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';

export function setupMonacoWorkers(): void {
  if (import.meta.env.RIDGE_WEB_REMOTE) {
    // §perf web-remote: 所有 label 统一走 editor.worker，跳过 ~9 MB 的语言 worker
    self.MonacoEnvironment = {
      getWorker() {
        return new editorWorker();
      },
    };
  } else {
    // Desktop Tauri: 按需加载各语言 worker（原有行为）
    void Promise.all([
      import('monaco-editor/esm/vs/language/json/json.worker?worker'),
      import('monaco-editor/esm/vs/language/css/css.worker?worker'),
      import('monaco-editor/esm/vs/language/html/html.worker?worker'),
      import('monaco-editor/esm/vs/language/typescript/ts.worker?worker'),
    ]).then(([jsonWorker, cssWorker, htmlWorker, tsWorker]) => {
      self.MonacoEnvironment = {
        getWorker(_: unknown, label: string) {
          if (label === 'json') return new jsonWorker.default();
          if (label === 'css' || label === 'scss' || label === 'less') return new cssWorker.default();
          if (label === 'html' || label === 'handlebars' || label === 'razor') return new htmlWorker.default();
          if (label === 'typescript' || label === 'javascript') return new tsWorker.default();
          return new editorWorker();
        },
      };
    });
  }
}
