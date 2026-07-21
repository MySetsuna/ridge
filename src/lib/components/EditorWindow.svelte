<script lang="ts">
  // src/lib/components/EditorWindow.svelte
  //
  // 「独立窗口」外壳：在独立 OS 窗口（?win=editor）里渲染整个文件编辑器，铺满窗口。
  // 由 +layout.svelte 在检测到 win=editor 时取代主应用渲染。
  //
  // 启动顺序与 +page.svelte 主应用对齐（仅取编辑器需要的部分）：
  //   1) initThemeSystem + initSettingsBoot → 把 --rg-* CSS 变量写到 documentElement
  //      （独立窗口不经主窗口的 splash init script，必须自己 bootstrap 主题）。
  //   2) initFileWatcherSync → 外部文件变更 / diff 实时刷新（idempotent）。
  //   3) 读取共享 localStorage 的交接快照，把标签（含未保存内容）直接载入本窗口 store。
  // FileEditor 在本组件里**无条件**挂载（主应用是 openFiles>0 才懒挂载；此处必须先于
  // 该门挂载，由 FileEditor 自身的 popout 分支铺满窗口）。
  // 同时挂载 ContextMenu / RidgeDialog 宿主——主应用在 +page 里挂，这里不经过 +page。

  import { onMount } from 'svelte';
  import { listen, emitTo, type UnlistenFn } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import FileEditor from './FileEditor.svelte';
  import ContextMenu from './ContextMenu.svelte';
  import WindDialog from './RidgeDialog.svelte';
  import { fileEditorStore, type OpenRequest } from '$lib/stores/fileEditor';
  import {
    EVT_OPEN,
    EVT_CLOSED,
    HANDOFF_KEY,
    type HandoffPayload,
  } from '$lib/stores/editorWindow';
  import { initThemeSystem } from '$lib/stores/themes';
  import { initSettingsBoot } from '$lib/stores/settings';
  import { initFileWatcherSync } from '$lib/stores/fileWatcherSync';

  /** 读取并载入交接快照（弹出时主窗口写入的打开文件列表）。 */
  function loadHandoff(): void {
    let raw: string | null = null;
    try {
      raw = localStorage.getItem(HANDOFF_KEY);
    } catch {
      /* localStorage 不可用：留空，由转发的 open 或用户操作填充 */
    }
    if (!raw) return;
    try {
      const payload = JSON.parse(raw) as HandoffPayload;
      if (payload?.files?.length) {
        fileEditorStore.loadFiles(payload.files, payload.active);
      }
    } catch (e) {
      console.warn('[EditorWindow] 解析交接快照失败', e);
    }
  }

  onMount(() => {
    let unlistenOpen: UnlistenFn | null = null;
    let unlistenClose: UnlistenFn | null = null;

    void (async () => {
      // 0) 先注册关闭请求处理器，再执行其他异步初始化。防止用户在主题/快照加载
      //    完成前就关闭窗口——若处理器未就绪，默认关闭行为不会 emit 快照给主窗口，
      //    导致主窗口的拦截器永久残留，后续无法在本地打开文件。
      unlistenClose = await getCurrentWindow().onCloseRequested(async (event) => {
        event.preventDefault();
        const snap = fileEditorStore.snapshot();
        try {
          await emitTo('main', EVT_CLOSED, snap);
        } catch (e) {
          console.warn('[EditorWindow] 交还快照失败', e);
        }
        await getCurrentWindow().destroy();
      });

      // 1) 主题：CSS 变量 bootstrap（独立窗口不经 splash init script）。
      await initThemeSystem();
      initSettingsBoot();
      // 2) 外部文件变更监听（idempotent）。
      initFileWatcherSync();
      // 3) 载入交接的标签。
      loadHandoff();

      // 主窗口转发来的新 open（弹出期间主窗口拦截并转发）。本窗口 store 无拦截器，
      // 正常本地打开。
      unlistenOpen = await listen<OpenRequest>(EVT_OPEN, (e) => {
        const req = e.payload;
        if (req.kind === 'file') void fileEditorStore.openFile(req.path, req.opts);
        else fileEditorStore.openDiffTab(req.args);
      });
    })();

    return () => {
      unlistenOpen?.();
      unlistenClose?.();
    };
  });
</script>

<!-- FileEditor 的 popout 分支让其 position:fixed inset:0 铺满窗口。
     ContextMenu / RidgeDialog 为编辑器的右键菜单与保存/确认对话框宿主。 -->
<FileEditor />
<ContextMenu />
<WindDialog />
