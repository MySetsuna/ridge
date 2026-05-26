<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { createRemoteConnection, type RemoteConnectionApi } from '$lib/remote/wsClient';
  import QrCode from '$lib/remote/QrCode.svelte';
  import TreeNodeRow from '$lib/remote/TreeNodeRow.svelte';
  import {
    Terminal,
    Menu,
    ChevronDown,
    MoreHorizontal,
    ArrowLeft,
    ArrowUp,
    File,
    Search,
    GitBranch,
    RefreshCw,
    FoldHorizontal,
    FileCode,
  } from 'lucide-svelte';

  // ── State ──
  let remoteInfo = $state<{ port: number; lanIp: string; totpCode: string; otpauthUri: string; ready: boolean } | null>(null);
  let hostInput = $state('localhost');
  let portInput = $state('');
  let manualCode = $state('');
  let draftInput = $state('');
  let screen: 'remote' | 'tools' = $state('remote');
  let activeTab: 'files' | 'search' | 'git' = $state('files');
  let activeTerminalTab = $state(0);
  let connected = $state(false);
  let terminalLines = $state<string[]>(['C:\\workcode\\myproject>']);
  let connectError = $state('');
  let totpTimer: ReturnType<typeof setInterval> | null = null;

  let conn = createRemoteConnection();

  function buildLinkUri(lanIp: string, port: number): string {
    return `http://${lanIp}:${port}/`;
  }

  async function refreshRemoteInfo(host: string, port: number) {
    try {
      const res = await fetch(`http://${host}:${port}/info`);
      if (!res.ok) return;
      const data = await res.json();
      remoteInfo = {
        port: data.port || port,
        lanIp: data.lanIp ?? data.lan_ip ?? host,
        totpCode: data.totpCode ?? data.totp_code,
        otpauthUri: data.otpauthUri ?? data.otpauth_uri,
        ready: true,
      };
    } catch { /* ignore */ }
  }

  onMount(() => {
    invoke<{ port: number; lanIp: string; totpCode: string; otpauthUri: string; ready: boolean }>('get_remote_info').then(info => {
      remoteInfo = info;
      portInput = String(info.port);
      hostInput = info.lanIp || 'localhost';
    }).catch(() => {
      // Not in Tauri — will discover via HTTP or manual input
    });
    totpTimer = setInterval(async () => {
      if (remoteInfo?.ready && hostInput && portInput) {
        await refreshRemoteInfo(hostInput, parseInt(portInput) || 0);
      }
    }, 5000);
    return () => { if (totpTimer) clearInterval(totpTimer); };
  });

  /** Fetch remote info from the Axum HTTP `/info` endpoint. */
  async function fetchRemoteInfo() {
    connectError = '';
    try {
      const host = hostInput || 'localhost';
      const port = parseInt(portInput) || 0;
      if (!port) { connectError = '请输入端口号'; return; }
      const res = await fetch(`http://${host}:${port}/info`);
      if (!res.ok) { connectError = `服务器返回 ${res.status}`; return; }
      const data = await res.json();
      remoteInfo = {
        port: data.port || port,
        lanIp: data.lanIp ?? data.lan_ip ?? host,
        totpCode: data.totpCode ?? data.totp_code,
        otpauthUri: data.otpauthUri ?? data.otpauth_uri,
        ready: true,
      };
      hostInput = remoteInfo.lanIp;
    } catch (e: unknown) {
      connectError = e instanceof Error ? e.message : '连接失败';
    }
  }

  // ── Connection helpers ──
  async function connectViaQR() {
    if (!remoteInfo?.ready) return;
    // Refresh the TOTP code before connecting (avoid stale code).
    await refreshRemoteInfo(hostInput || 'localhost', remoteInfo.port);
    if (!remoteInfo?.totpCode) return;
    connected = true;
    conn.connect(hostInput || 'localhost', remoteInfo.port, remoteInfo.totpCode);
  }

  function connectManually() {
    if (!remoteInfo?.ready || !hostInput || !manualCode) return;
    connected = true;
    conn.connect(hostInput, remoteInfo.port, manualCode);
  }

  function sendCommand() {
    if (!draftInput.trim()) return;
    terminalLines = [...terminalLines, `C:\\workcode\\myproject> ${draftInput}`];
    draftInput = '';
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      sendCommand();
    }
  }

  // ── Time display ──
  let now = $state(new Date());
  $effect(() => {
    const t = setInterval(() => now = new Date(), 10000);
    return () => clearInterval(t);
  });
  const timeStr = $derived(
    now.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false })
  );

  interface TreeNode {
    name: string;
    type: 'dir' | 'file';
    children: TreeNode[];
  }

  // Mock file tree
  const fileTree: TreeNode[] = [
    { name: '.git', type: 'dir', children: [] },
    { name: 'src', type: 'dir', children: [
      { name: 'components', type: 'dir', children: [
        { name: 'Button.vue', type: 'file', children: [] },
        { name: 'Input.vue', type: 'file', children: [] },
      ]},
      { name: 'utils', type: 'dir', children: [
        { name: 'request.js', type: 'file', children: [] },
      ]},
      { name: 'App.vue', type: 'file', children: [] },
      { name: 'main.js', type: 'file', children: [] },
    ]},
    { name: 'public', type: 'dir', children: [] },
    { name: 'package.json', type: 'file', children: [] },
    { name: 'README.md', type: 'file', children: [] },
  ];

  const searchResults = [
    { file: 'Button.vue', path: '../src/components/Button.vue', matches: 12 },
    { file: 'Input.vue', path: '../src/components/Input.vue', matches: 8 },
    { file: 'request.js', path: '../src/utils/request.js', matches: 5 },
    { file: 'App.vue', path: '../src/App.vue', matches: 3 },
    { file: 'README.md', path: '../README.md', matches: 1 },
  ];

  const gitCommits = [
    { msg: 'feat: 添加登录功能', hash: 'a1b2c3d', time: '2 小时前' },
    { msg: 'fix: 修复按钮样式问题', hash: 'e4f5g6h', time: '5 小时前' },
    { msg: 'docs: 更新 README', hash: 'i7j8k9l', time: '1 天前' },
    { msg: 'feat: 初始化项目', hash: 'm0n1o2p', time: '2 天前' },
  ];

  let fileTreeExpanded = $state<Record<string, boolean>>({ '.git': true, 'src': true, 'components': true, 'utils': true, 'public': true });

  function toggleDir(name: string) {
    fileTreeExpanded[name] = !fileTreeExpanded[name];
  }

  const fileCount = $derived.by(() => {
    const files: string[] = [];
    function walk(nodes: TreeNode[]) {
      for (const n of nodes) {
        if (n.type === 'file') files.push(n.name);
        walk(n.children);
      }
    }
    walk(fileTree);
    return files.length;
  });
</script>

<div class="fixed inset-0 bg-[var(--rg-bg)] flex flex-col overflow-hidden">
  {#if screen === 'remote'}
    {#if !connected}
      <div class="flex flex-col items-center justify-center flex-1 px-6 gap-6">
        <div class="w-16 h-16 rounded-2xl bg-[var(--rg-accent)]/10 flex items-center justify-center">
          <Terminal class="w-8 h-8 text-[var(--rg-accent)]" />
        </div>
        <h1 class="text-xl font-semibold text-[var(--rg-fg)]">远程终端</h1>
        <p class="text-sm text-[var(--rg-fg-muted)] text-center max-w-xs">
          扫码连接桌面 Ridge 终端，或在局域网内手动连接
        </p>

        {#if remoteInfo?.ready}
          <div class="flex flex-col items-center gap-2">
            <p class="text-xs text-[var(--rg-fg-muted)]">① 扫码绑定身份验证器</p>
            <QrCode value={remoteInfo.otpauthUri} size={140} />
          </div>
          <div class="flex flex-col items-center gap-2">
            <p class="text-xs text-[var(--rg-fg-muted)]">② 扫码打开远程页面</p>
            <QrCode value={buildLinkUri(remoteInfo.lanIp, remoteInfo.port)} size={140} />
            <p class="text-[10px] text-[var(--rg-fg-muted)]">手机浏览器扫码 → 输入验证码 → 连接</p>
          </div>

          <div class="flex items-center gap-3 w-full max-w-xs my-2">
            <div class="flex-1 h-px bg-[var(--rg-border)]"></div>
            <span class="text-xs text-[var(--rg-fg-muted)]">或</span>
            <div class="flex-1 h-px bg-[var(--rg-border)]"></div>
          </div>
        {/if}

        <div class="w-full max-w-xs space-y-3">
          <div class="flex gap-2">
            <input
              bind:value={hostInput}
              placeholder="localhost"
              class="flex-1 h-10 px-4 rounded-lg bg-[var(--rg-surface)] border border-[var(--rg-border)] text-sm text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors"
            />
            <input
              bind:value={portInput}
              placeholder="端口"
              class="w-24 h-10 px-3 rounded-lg bg-[var(--rg-surface)] border border-[var(--rg-border)] text-sm text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors"
            />
          </div>
          <button
            onclick={connectViaQR}
            class="w-full h-10 rounded-lg bg-[var(--rg-accent)] text-white text-sm font-medium transition-opacity"
          >
            {remoteInfo?.totpCode ?? '------'}
          </button>
          <div class="flex gap-2">
            <input
              bind:value={manualCode}
              placeholder="TOTP 验证码"
              maxlength={6}
              class="flex-1 h-10 px-4 rounded-lg bg-[var(--rg-surface)] border border-[var(--rg-border)] text-sm text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors"
            />
            <button
              onclick={connectManually}
              disabled={!hostInput || manualCode.length < 6}
              class="shrink-0 h-10 px-4 rounded-lg bg-[var(--rg-accent)] text-white text-sm font-medium disabled:opacity-40 transition-opacity"
            >
              连接
            </button>
          </div>
          <button
            onclick={fetchRemoteInfo}
            class="w-full h-10 rounded-lg border border-dashed border-[var(--rg-border)] text-[var(--rg-fg-muted)] text-sm hover:bg-[var(--rg-surface)] transition-colors"
          >
            获取服务器信息
          </button>
          {#if connectError}
            <p class="text-xs text-red-400">{connectError}</p>
          {/if}
        </div>
      </div>
    {:else}
      {#if conn}
        <!-- Status bar -->
        <div class="flex items-center justify-between h-7 px-4 bg-[var(--rg-surface)] text-[10px] text-[var(--rg-fg-muted)] shrink-0">
          <span>{timeStr}</span>
          <span>5G</span>
          <span>🔋 20%</span>
        </div>

        <!-- Header -->
        <div class="flex items-center justify-between h-11 px-3 border-b border-[var(--rg-border)] shrink-0">
          <button onclick={() => { connected = false; conn.disconnect(); }} class="p-1.5 hover:bg-[var(--rg-surface)] rounded-lg transition-colors">
            <Menu class="w-5 h-5 text-[var(--rg-fg)]" />
          </button>
          <div class="flex items-center gap-1">
            <span class="text-sm font-medium text-[var(--rg-fg)]">移动终端</span>
            <ChevronDown class="w-3.5 h-3.5 text-[var(--rg-fg-muted)]" />
          </div>
          <span></span>
        </div>

        <!-- Terminal pane -->
        <div class="flex-1 flex flex-col p-3 min-h-0">
          <div class="flex-1 rounded-xl border border-[var(--rg-border)] bg-black/5 dark:bg-white/5 overflow-hidden flex flex-col">
            <div class="flex items-center gap-1.5 px-3 py-2 bg-[var(--rg-surface)]/50">
              <span class="w-2.5 h-2.5 rounded-full bg-red-400"></span>
              <span class="w-2.5 h-2.5 rounded-full bg-yellow-400"></span>
              <span class="w-2.5 h-2.5 rounded-full bg-green-400"></span>
              <span class="ml-2 text-[10px] text-[var(--rg-fg-muted)]">terminal — ridge-remote</span>
            </div>
            <div class="flex-1 overflow-auto p-3">
              {#each terminalLines as line, i}
                <pre class="text-sm font-mono leading-relaxed text-[var(--rg-fg)] whitespace-pre-wrap">{line}</pre>
              {/each}
            </div>
          </div>
        </div>

        <!-- Draft input -->
        <div class="shrink-0 px-3 pb-2">
          <div class="relative">
            <input
              bind:value={draftInput}
              onkeydown={handleKeydown}
              placeholder="文本输入....(回车发送)"
              class="w-full h-11 pl-4 pr-12 rounded-xl bg-[var(--rg-surface)] border border-[var(--rg-border)] text-sm text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors"
            />
            <button
              onclick={sendCommand}
              disabled={!draftInput.trim()}
              class="absolute right-1.5 top-1/2 -translate-y-1/2 w-8 h-8 flex items-center justify-center rounded-lg bg-[var(--rg-accent)] text-white disabled:opacity-30 transition-opacity"
            >
              <ArrowUp class="w-4 h-4" />
            </button>
          </div>
          <p class="mt-1 text-[10px] text-[var(--rg-fg-muted)] px-1">
            上下两个输入区域是同步的，按发送 = 回车
          </p>
        </div>

        <!-- Bottom tab bar -->
        <div class="shrink-0 flex border-t border-[var(--rg-border)] bg-[var(--rg-surface)]">
          {#each ['终端1', '终端2', '终端3'] as tab, i}
            <button
              onclick={() => activeTerminalTab = i}
              class="flex-1 h-11 text-xs font-medium transition-colors relative"
              class:text-[var(--rg-accent)]={activeTerminalTab === i}
              class:text-[var(--rg-fg-muted)]={activeTerminalTab !== i}
            >
              {tab}
              {#if activeTerminalTab === i}
                <div class="absolute bottom-0 left-1/4 right-1/4 h-0.5 bg-[var(--rg-accent)] rounded-full"></div>
              {/if}
            </button>
          {/each}
          <button
            onclick={() => { screen = 'tools'; }}
            class="flex-1 h-11 text-xs font-medium text-[var(--rg-fg-muted)] transition-colors flex items-center justify-center gap-1"
          >
            工作区1
            <ChevronDown class="w-3 h-3" />
          </button>
        </div>
      {/if}
    {/if}
  {:else}
    <!-- Screen 2: Project Tools -->
    <!-- Header -->
    <div class="flex items-center justify-between h-11 px-3 border-b border-[var(--rg-border)] shrink-0">
      <button onclick={() => screen = 'remote'} class="flex items-center gap-1 p-1.5 hover:bg-[var(--rg-surface)] rounded-lg transition-colors">
        <ArrowLeft class="w-5 h-5 text-[var(--rg-fg)]" />
      </button>
      <span class="text-sm font-medium text-[var(--rg-fg)]">我的项目 - myproject</span>
      <MoreHorizontal class="w-5 h-5 text-[var(--rg-fg-muted)]" />
    </div>

    <!-- Body: vertical tabs + content -->
    <div class="flex-1 flex min-h-0">
      <!-- Vertical Tab Bar -->
      <div class="w-16 shrink-0 flex flex-col items-center gap-2 pt-3 border-r border-[var(--rg-border)] bg-[var(--rg-surface)]/50">
        <button
          onclick={() => activeTab = 'files'}
          class="w-12 h-12 rounded-xl flex flex-col items-center justify-center gap-0.5 transition-colors {(activeTab === 'files') ? 'bg-[var(--rg-accent)]/10 text-[var(--rg-accent)]' : 'text-[var(--rg-fg-muted)]'}"
        >
          <File class="w-5 h-5" />
          <span class="text-[9px]">文件</span>
        </button>
        <button
          onclick={() => activeTab = 'search'}
          class="w-12 h-12 rounded-xl flex flex-col items-center justify-center gap-0.5 transition-colors {(activeTab === 'search') ? 'bg-[var(--rg-accent)]/10 text-[var(--rg-accent)]' : 'text-[var(--rg-fg-muted)]'}"
        >
          <Search class="w-5 h-5" />
          <span class="text-[9px]">搜索</span>
        </button>
        <button
          onclick={() => activeTab = 'git'}
          class="w-12 h-12 rounded-xl flex flex-col items-center justify-center gap-0.5 transition-colors {(activeTab === 'git') ? 'bg-[var(--rg-accent)]/10 text-[var(--rg-accent)]' : 'text-[var(--rg-fg-muted)]'}"
        >
          <GitBranch class="w-5 h-5" />
          <span class="text-[9px]">Git</span>
        </button>
      </div>

      <!-- Panel Content -->
      <div class="flex-1 overflow-auto">
        {#if activeTab === 'files'}
          <div class="p-3">
            <div class="flex items-center justify-between mb-3">
              <h2 class="text-sm font-medium text-[var(--rg-fg)]">文件树</h2>
              <div class="flex items-center gap-1">
                <FoldHorizontal class="w-4 h-4 text-[var(--rg-fg-muted)]" />
                <RefreshCw class="w-4 h-4 text-[var(--rg-fg-muted)]" />
              </div>
            </div>
            <div class="space-y-0.5">
              {#each fileTree as node}
                <TreeNodeRow {node} expanded={fileTreeExpanded} onToggle={toggleDir} depth={0} />
              {/each}
            </div>
            <div class="mt-4 text-[10px] text-[var(--rg-fg-muted)]">
              共 {fileCount} 个文件，{fileTree.filter(n => n.type === 'dir').length} 个文件夹
            </div>
          </div>
        {:else if activeTab === 'search'}
          <div class="p-3">
            <div class="relative mb-4">
              <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--rg-fg-muted)]" />
              <input
                placeholder="搜索文件内容..."
                class="w-full h-10 pl-10 pr-4 rounded-lg bg-[var(--rg-surface)] border border-[var(--rg-border)] text-sm text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)] transition-colors"
              />
            </div>
            <div class="space-y-2">
              {#each searchResults as r}
                <div class="p-2 rounded-lg hover:bg-[var(--rg-surface)] transition-colors cursor-pointer">
                  <div class="flex items-center justify-between">
                    <span class="text-sm font-medium text-[var(--rg-fg)]">{r.file}</span>
                    <span class="text-[10px] px-1.5 py-0.5 rounded bg-[var(--rg-accent)]/10 text-[var(--rg-accent)]">{r.matches} 行匹配</span>
                  </div>
                  <p class="text-[11px] text-[var(--rg-fg-muted)] mt-0.5">{r.path}</p>
                </div>
              {/each}
            </div>
            <div class="mt-4 text-[10px] text-[var(--rg-fg-muted)]">
              共找到 {searchResults.length} 个结果
            </div>
          </div>
        {:else if activeTab === 'git'}
          <div class="p-3 space-y-4">
            <!-- Staged / Unstaged -->
            <div>
              <h3 class="text-xs font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider mb-2">已 staged ②</h3>
              {#each ['src/components/Button.vue · M', 'package.json · M'] as item}
                <div class="flex items-center gap-2 py-1.5 px-2 rounded-lg hover:bg-[var(--rg-surface)] text-sm text-[var(--rg-fg)]">
                  <FileCode class="w-4 h-4 text-green-400 shrink-0" />
                  <span class="truncate">{item}</span>
                </div>
              {/each}
            </div>
            <div>
              <h3 class="text-xs font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider mb-2">未 staged ③</h3>
              {#each [{ n: 'src/utils/request.js', s: 'M' }, { n: 'README.md', s: 'A' }, { n: 'public/logo.png', s: 'D' }] as item}
                <div class="flex items-center gap-2 py-1.5 px-2 rounded-lg hover:bg-[var(--rg-surface)] text-sm text-[var(--rg-fg)]">
                  <FileCode class="w-4 h-4 text-[var(--rg-fg-muted)] shrink-0" />
                  <span class="truncate">{item.n}</span>
                  <span class="text-[10px] px-1 rounded bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] ml-auto">{item.s}</span>
                </div>
              {/each}
            </div>

            <!-- Git Graph -->
            <div>
              <div class="flex items-center justify-between mb-2">
                <h3 class="text-xs font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider">Git Graph</h3>
                <div class="flex items-center gap-1">
                  <span class="text-[10px] px-1.5 py-0.5 rounded bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] cursor-pointer">main ▼</span>
                  <span class="text-[var(--rg-fg-muted)] cursor-pointer">🔍+</span>
                  <span class="text-[var(--rg-fg-muted)] cursor-pointer">⤢</span>
                </div>
              </div>
              <div class="space-y-1">
                {#each gitCommits as c, i}
                  <div class="flex items-start gap-2 py-1">
                    <div class="flex flex-col items-center shrink-0">
                      <div class="w-2.5 h-2.5 rounded-full bg-[var(--rg-accent)] mt-1.5"></div>
                      {#if i < gitCommits.length - 1}
                        <div class="w-px h-5 bg-[var(--rg-border)]"></div>
                      {/if}
                    </div>
                    <div class="min-w-0">
                      <p class="text-sm text-[var(--rg-fg)] truncate">{c.msg}</p>
                      <p class="text-[10px] text-[var(--rg-fg-muted)] space-x-2">
                        <span>{c.hash}</span>
                        <span>{c.time}</span>
                      </p>
                    </div>
                  </div>
                {/each}
              </div>
            </div>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>


