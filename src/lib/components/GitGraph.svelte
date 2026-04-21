<script lang="ts">
import { onMount } from 'svelte';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { activeWorkspaceId, paneCwdStore, activePaneId } from '$lib/stores/paneTree';

interface CommitNode {
  hash: string;
  subject: string;
  author: string;
  date: string;
  parents: string[];
  branch?: string;
}

interface DiffFile {
  path: string;
  additions: number;
  deletions: number;
  status: string;
}

interface GitRepoInfo {
  is_git_repo: boolean;
  commits: CommitNode[];
  branches: string[];
  current_branch: string | null;
  diff: {
    files: DiffFile[];
    total_additions: number;
    total_deletions: number;
    is_git_repo: boolean;
  };
}

let gitInfo: GitRepoInfo | null = $state(null);
let loading = $state(true);
let error = $state<string | null>(null);
let selectedCommit: CommitNode | null = $state(null);
let cwd: string | undefined = $state(undefined);

// 从 store 获取当前的 cwd
function getCurrentCwd(): string | undefined {
  const wsId = get(activeWorkspaceId);
  const pId = get(activePaneId);
  if (wsId && pId) {
    return get(paneCwdStore)[`${wsId}:${pId}`];
  }
  return undefined;
}

// 尝试加载 git 信息的函数
function tryLoadGitInfo() {
  cwd = getCurrentCwd();
  if (cwd) {
    void loadGitInfo();
  }
}

onMount(() => {
  // 初始加载
  tryLoadGitInfo();

  // 订阅变化 - activePaneId, paneCwdStore, activeWorkspaceId
  const unsub1 = activePaneId.subscribe(() => {
    tryLoadGitInfo();
  });
  const unsub2 = paneCwdStore.subscribe(() => {
    tryLoadGitInfo();
  });
  const unsub3 = activeWorkspaceId.subscribe(() => {
    // 工作区切换时也尝试重新加载
    tryLoadGitInfo();
  });

  return () => {
    unsub1();
    unsub2();
    unsub3();
  };
});

async function loadGitInfo() {
  if (!cwd || !isTauri()) {
    loading = false;
    return;
  }

  loading = true;
  error = null;
  try {
    gitInfo = await invoke<GitRepoInfo>('get_git_info_with_cwd', { cwd });
  } catch (e) {
    error = String(e);
  } finally {
    loading = false;
  }
}

function handleCommitClick(commit: CommitNode) {
  selectedCommit = commit;
}

// 构建树形文件结构
function buildFileTree(files: DiffFile[]): { name: string; path: string; type: 'file' | 'dir'; children?: any[] }[] {
  const root: any = {};

  for (const file of files) {
    const parts = file.path.split('/');
    let current = root;

    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      const isLast = i === parts.length - 1;

      if (isLast) {
        current[part] = { ...file, type: 'file' as const };
      } else {
        if (!current[part]) {
          current[part] = { name: part, type: 'dir' as const, children: {} };
        }
        current = current[part].children;
      }
    }
  }

  // 转换为数组
  function toArray(obj: any, path: string = ''): any[] {
    return Object.entries(obj).map(([name, value]: [string, any]) => {
      const fullPath = path ? `${path}/${name}` : name;
      if (value.type === 'dir') {
        return {
          name,
          path: fullPath,
          type: 'dir',
          children: toArray(value.children || {}, fullPath)
        };
      }
      return {
        name,
        path: fullPath,
        type: 'file',
        status: value.status,
        additions: value.additions,
        deletions: value.deletions
      };
    });
  }

  return toArray(root);
}

function getStatusColor(status: string): string {
  switch (status) {
    case 'M': return 'text-yellow-400';
    case 'A': return 'text-green-400';
    case 'D': return 'text-red-400';
    case 'R': return 'text-purple-400';
    case 'C': return 'text-blue-400';
    default: return 'text-gray-400';
  }
}

function formatDate(timestamp: string): string {
  const date = new Date(parseInt(timestamp) * 1000);
  return date.toLocaleDateString('zh-CN', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit'
  });
}

function getStatusLabel(status: string): string {
  switch (status) {
    case 'M': return '修改';
    case 'A': return '新增';
    case 'D': return '删除';
    case 'R': return '重命名';
    case 'C': return '复制';
    default: return status;
  }
}
</script>

<div class="gitgraph-wrapper">
  <!-- 工具栏 -->
  <div class="flex items-center gap-2 mb-3">
    <button
      type="button"
      class="flex-1 rounded-lg px-3 py-1.5 text-[12px] font-medium bg-[var(--wf-surface)] border border-[var(--wf-border)] hover:border-violet-400/25 hover:bg-violet-500/[0.06] transition-colors"
      onclick={() => loadGitInfo()}
    >
      刷新
    </button>
    {#if gitInfo?.current_branch}
      <span class="text-[11px] px-2 py-1 rounded bg-green-500/20 text-green-400">
        {gitInfo.current_branch}
      </span>
    {/if}
  </div>

  <!-- Git 仓库信息头部 -->
  {#if gitInfo && gitInfo.is_git_repo}
    <div class="mb-3 p-2 rounded-lg bg-[var(--wf-surface)] border border-[var(--wf-border)] flex items-center justify-between">
      <span class="text-[12px] text-[var(--wf-fg)]">
        📁 {cwd}
      </span>
      <span class="text-[11px] text-[var(--wf-fg-muted)]">
        {gitInfo.branches.length} 个分支
      </span>
    </div>
  {/if}

  {#if loading}
    <div class="mt-4 text-center text-[12px] text-[var(--wf-fg-muted)]">
      加载中...
    </div>
  {:else if error}
    <div class="mt-4 p-3 rounded-lg bg-red-500/10 border border-red-500/20">
      <p class="text-[12px] text-red-400">错误: {error}</p>
    </div>
  {:else if !gitInfo?.is_git_repo}
    <div class="mt-4 p-4 rounded-lg bg-[var(--wf-surface)] border border-[var(--wf-border)] text-center">
      <p class="text-[13px] text-[var(--wf-fg-muted)]">
        当前目录不是 Git 仓库
      </p>
      <p class="text-[11px] text-[var(--wf-fg-muted)] mt-1">
        切换到 Git 仓库目录以查看 Git 信息
      </p>
    </div>
  {:else}
    <!-- Git Graph 和 Changes 分栏显示 -->
    <div class="flex gap-3">
      <!-- 左侧：提交历史 -->
      <div class="flex-1">
        <h4 class="text-[12px] font-medium text-[var(--wf-fg-muted)] mb-2">
          提交历史 ({gitInfo?.commits.length || 0})
        </h4>
        <div class="space-y-1 max-h-[300px] overflow-y-auto">
          {#each gitInfo?.commits || [] as commit}
            <button
              type="button"
              class="w-full text-left px-3 py-2 rounded-lg hover:bg-[var(--wf-surface)] transition-colors"
              onclick={() => handleCommitClick(commit)}
            >
              <div class="flex items-center gap-2">
                <span class="text-[10px] font-mono text-violet-400">
                  {commit.hash.slice(0, 7)}
                </span>
                {#if commit.branch}
                  <span class="text-[10px] px-1.5 py-0.5 rounded bg-green-500/20 text-green-400">
                    {commit.branch}
                  </span>
                {/if}
              </div>
              <div class="text-[12px] text-[var(--wf-fg)] mt-0.5 truncate">
                {commit.subject}
              </div>
              <div class="text-[10px] text-[var(--wf-fg-muted)] mt-0.5">
                {commit.author} · {formatDate(commit.date)}
              </div>
            </button>
          {/each}
        </div>
      </div>

      <!-- 右侧：文件改动 -->
      <div class="flex-1">
        <h4 class="text-[12px] font-medium text-[var(--wf-fg-muted)] mb-2">
          文件改动 ({gitInfo?.diff.files.length || 0})
          <span class="text-[10px] text-green-400 ml-1">
            +{gitInfo?.diff.total_additions || 0}
          </span>
          <span class="text-[10px] text-red-400 ml-1">
            -{gitInfo?.diff.total_deletions || 0}
          </span>
        </h4>
        <div class="space-y-1 max-h-[300px] overflow-y-auto">
          {#each gitInfo?.diff.files || [] as file}
            <div class="flex items-center gap-2 px-2 py-1.5 rounded hover:bg-[var(--wf-surface)] transition-colors text-[11px]">
              <span class={getStatusColor(file.status)}>
                {getStatusLabel(file.status)}
              </span>
              <span class="text-[var(--wf-fg)] truncate flex-1">
                {file.path}
              </span>
              <span class="text-green-400 text-[10px]">
                +{file.additions}
              </span>
              <span class="text-red-400 text-[10px]">
                -{file.deletions}
              </span>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
.gitgraph-wrapper {
  padding: 0.5rem;
}
</style>