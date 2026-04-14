<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import {
    contextMenu,
    showContextMenu,
    hideContextMenu,
  } from '$lib/stores/contextMenu';
  import { createGitgraph, templateExtend, TemplateName } from '@gitgraph/js';

  interface CommitNode {
    hash: string;
    subject: string;
    author: string;
    date: string;
    parents: string[];
    branch?: string;
  }

  let gitgraphContainer: HTMLDivElement;
  let commits: CommitNode[] = $state([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let selectedCommit: CommitNode | null = $state(null);

  // GitHub 配置
  let githubToken = $state('');
  let repoOwner = $state('');
  let repoName = $state('');
  let showSettings = $state(false);

  onMount(() => {
    initGitGraph();
    void loadGraph();
  });

  function initGitGraph() {
    if (!gitgraphContainer) return;

    // 清除之前的内容
    gitgraphContainer.innerHTML = '';

    const gitgraph = createGitgraph(gitgraphContainer, {
      template: templateExtend(TemplateName.Metro, {
        colors: [
          '#0fbcf9', // cyan
          '#4ade80', // green
          '#f472b6', // pink
          '#fbbf24', // amber
          '#a78bfa', // violet
          '#2dd4bf', // teal
        ],
        branch: {
          lineWidth: 2,
          spacing: 40,
          label: {
            font: '12px JetBrains Mono, monospace',
            color: '#e6e4ef',
            bgColor: '#1a1628',
            borderRadius: 4,
          },
        },
        commit: {
          message: {
            font: '11px JetBrains Mono, monospace',
            color: '#9ca3af',
            displayAuthor: false,
          },
        },
      }),
      author: 'WarpForge User <user@warpforge.local>',
      branchLabelOnEveryCommit: false,
    });

    // 添加示例分支
    const main = gitgraph.branch('main');
    main.commit({ subject: 'Initial commit', hash: 'abc1234' });
    main.commit({ subject: 'Add project structure', hash: 'def5678' });

    const feature = gitgraph.branch('feat/new-feature');
    feature.commit({ subject: 'Implement feature', hash: 'ghi9012' });
    feature.commit({ subject: 'Fix bug in feature', hash: 'jkl3456' });

    main.commit({ subject: 'Merge feature branch', hash: 'mno7890' });
  }

  async function loadGraph() {
    loading = true;
    error = null;
    try {
      if (isTauri()) {
        commits = await invoke<CommitNode[]>('get_git_graph', {
          repoPath: '.',
        });
      }
      // 演示数据
      if (commits.length === 0) {
        commits = [
          {
            hash: 'abc1234',
            subject: 'Initial commit',
            author: 'User',
            date: '2024-01-01',
            parents: [],
          },
          {
            hash: 'def5678',
            subject: 'Add project structure',
            author: 'User',
            date: '2024-01-02',
            parents: ['abc1234'],
          },
          {
            hash: 'ghi9012',
            subject: 'Implement feature',
            author: 'User',
            date: '2024-01-03',
            parents: ['def5678'],
            branch: 'feat/new-feature',
          },
        ];
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function handleContextMenu(event: MouseEvent, commit: CommitNode) {
    event.preventDefault();
    selectedCommit = commit;
    contextMenu.show(event.clientX, event.clientY, [
      {
        id: 'copy-hash',
        label: '复制 Commit Hash',
        shortcut: 'Ctrl+C',
        action: () => {
          navigator.clipboard.writeText(commit.hash);
          contextMenu.hide();
        },
      },
      {
        id: 'copy-full-hash',
        label: '复制完整 Hash',
        action: () => {
          navigator.clipboard.writeText(commit.hash);
          contextMenu.hide();
        },
      },
      { id: 'divider1', label: '', divider: true, action: () => {} },
      {
        id: 'github-open',
        label: '在 GitHub 上打开',
        action: () => {
          if (repoOwner && repoName) {
            const url = `https://github.com/${repoOwner}/${repoName}/commit/${commit.hash}`;
            window.open(url, '_blank');
          } else {
            alert('请先在设置中配置 GitHub 仓库信息');
            showSettings = true;
          }
          contextMenu.hide();
        },
      },
      {
        id: 'github-branch',
        label: '查看所在分支',
        action: () => {
          if (repoOwner && repoName && commit.branch) {
            const url = `https://github.com/${repoOwner}/${repoName}/tree/${commit.branch}`;
            window.open(url, '_blank');
          }
          contextMenu.hide();
        },
      },
      { id: 'divider2', label: '', divider: true, action: () => {} },
      {
        id: 'view-details',
        label: '查看详情',
        action: () => {
          alert(
            `Commit: ${commit.hash}\nAuthor: ${commit.author}\nDate: ${commit.date}\nMessage: ${commit.subject}`,
          );
          contextMenu.hide();
        },
      },
    ]);
  }

  function saveGithubSettings() {
    localStorage.setItem('wind-github-token', githubToken);
    localStorage.setItem('wind-github-owner', repoOwner);
    localStorage.setItem('wind-github-repo', repoName);
    showSettings = false;
    alert('GitHub 设置已保存');
  }

  function loadGithubSettings() {
    githubToken = localStorage.getItem('wind-github-token') || '';
    repoOwner = localStorage.getItem('wind-github-owner') || '';
    repoName = localStorage.getItem('wind-github-repo') || '';
  }
</script>

<div class="gitgraph-wrapper">
  <!-- 工具栏 -->
  <div class="flex items-center gap-2 mb-3">
    <button
      type="button"
      class="flex-1 rounded-lg px-3 py-1.5 text-[12px] font-medium bg-[var(--wf-surface)] border border-[var(--wf-border)] hover:border-violet-400/25 hover:bg-violet-500/[0.06] transition-colors"
      onclick={() => loadGraph()}
    >
      刷新
    </button>
    <button
      type="button"
      class="rounded-lg px-3 py-1.5 text-[12px] font-medium bg-[var(--wf-surface)] border border-[var(--wf-border)] hover:border-violet-400/25 hover:bg-violet-500/[0.06] transition-colors"
      onclick={() => {
        loadGithubSettings();
        showSettings = !showSettings;
      }}
    >
      GitHub 设置
    </button>
  </div>

  <!-- GitHub 设置面板 -->
  {#if showSettings}
    <div
      class="mb-3 p-3 rounded-lg bg-[var(--wf-surface)] border border-[var(--wf-border)]"
    >
      <h4 class="text-[13px] font-medium text-[var(--wf-fg)] mb-2">
        GitHub 配置
      </h4>
      <div class="space-y-2">
        <div>
          <label class="block text-[11px] text-[var(--wf-fg-muted)] mb-1"
            >仓库所有者 (Owner)</label
          >
          <input
            type="text"
            bind:value={repoOwner}
            placeholder="e.g. username"
            class="w-full px-2 py-1 text-[12px] rounded bg-[var(--wf-term-bg)] border border-[var(--wf-border)] text-[var(--wf-fg)] focus:border-violet-400/50 outline-none"
          />
        </div>
        <div>
          <label class="block text-[11px] text-[var(--wf-fg-muted)] mb-1"
            >仓库名称 (Repo)</label
          >
          <input
            type="text"
            bind:value={repoName}
            placeholder="e.g. my-project"
            class="w-full px-2 py-1 text-[12px] rounded bg-[var(--wf-term-bg)] border border-[var(--wf-border)] text-[var(--wf-fg)] focus:border-violet-400/50 outline-none"
          />
        </div>
        <div>
          <label class="block text-[11px] text-[var(--wf-fg-muted)] mb-1"
            >Personal Access Token (可选)</label
          >
          <input
            type="password"
            bind:value={githubToken}
            placeholder="ghp_xxxx..."
            class="w-full px-2 py-1 text-[12px] rounded bg-[var(--wf-term-bg)] border border-[var(--wf-border)] text-[var(--wf-fg)] focus:border-violet-400/50 outline-none"
          />
        </div>
        <button
          type="button"
          class="w-full mt-2 rounded-lg px-3 py-1.5 text-[12px] font-medium bg-violet-600 hover:bg-violet-500 text-white transition-colors"
          onclick={saveGithubSettings}
        >
          保存设置
        </button>
      </div>
    </div>
  {/if}

  <!-- GitGraph 画布 -->
  <div
    bind:this={gitgraphContainer}
    class="gitgraph-canvas rounded-lg overflow-hidden"
    style="background: var(--wf-term-bg); min-height: 300px;"
  ></div>

  <!-- Commit 列表 (备用显示) -->
  <div class="mt-3 space-y-1">
    <h4 class="text-[12px] font-medium text-[var(--wf-fg-muted)] mb-2">
      提交历史
    </h4>
    {#each commits as commit}
      <button
        type="button"
        class="w-full text-left px-3 py-2 rounded-lg hover:bg-[var(--wf-surface)] transition-colors"
        onclick={(e) => handleContextMenu(e as MouseEvent, commit)}
        oncontextmenu={(e) => handleContextMenu(e, commit)}
      >
        <div class="flex items-center gap-2">
          <span class="text-[10px] font-mono text-violet-400"
            >{commit.hash.slice(0, 7)}</span
          >
          {#if commit.branch}
            <span
              class="text-[10px] px-1.5 py-0.5 rounded bg-green-500/20 text-green-400"
            >
              {commit.branch}
            </span>
          {/if}
        </div>
        <div class="text-[12px] text-[var(--wf-fg)] mt-0.5 truncate">
          {commit.subject}
        </div>
        <div class="text-[10px] text-[var(--wf-fg-muted)] mt-0.5">
          {commit.author} · {commit.date}
        </div>
      </button>
    {/each}
  </div>

  {#if loading}
    <div class="mt-4 text-center text-[12px] text-[var(--wf-fg-muted)]">
      加载中...
    </div>
  {/if}

  {#if error}
    <div class="mt-4 p-3 rounded-lg bg-red-500/10 border border-red-500/20">
      <p class="text-[12px] text-red-400">错误: {error}</p>
    </div>
  {/if}

  {#if commits.length === 0 && !loading}
    <p class="mt-3 text-[11px] leading-relaxed text-[var(--wf-fg-muted)]">
      后端尚未返回提交数据（当前为本地演示数据）。配置 GitHub
      后可获取真实仓库数据。
    </p>
  {/if}
</div>

<style>
  .gitgraph-wrapper {
    padding: 0.5rem;
  }

  .gitgraph-canvas :global(svg) {
    max-width: 100%;
    height: auto;
  }

  .gitgraph-canvas :global(.gitgraph) {
    font-family: 'JetBrains Mono', monospace;
  }

  .gitgraph-canvas :global(.gitgraph .branch-label) {
    font-size: 11px;
  }
</style>
