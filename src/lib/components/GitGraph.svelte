<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';

  type CommitNode = { lane: number; parents: number[]; message: string };

  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D;
  let commits: CommitNode[] = [];

  onMount(() => {
    ctx = canvas.getContext('2d')!;
    drawGraph();
    void loadGraph();
  });

  async function loadGraph() {
    if (!isTauri()) return;
    commits = await invoke<CommitNode[]>('get_git_graph', { repoPath: '.' });
    drawGraph();
  }

  function drawGraph() {
    if (!ctx) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    const laneWidth = 40;
    commits.forEach((commit, i) => {
      const y = i * 60 + 30;
      // 画点
      ctx.fillStyle = '#0f0';
      ctx.beginPath();
      ctx.arc(30 + commit.lane * laneWidth, y, 8, 0, Math.PI * 2);
      ctx.fill();

      // 画线到 parent
      commit.parents.forEach((parentIdx: number) => {
        ctx.strokeStyle = '#666';
        ctx.beginPath();
        ctx.moveTo(30 + commit.lane * laneWidth, y);
        ctx.lineTo(30 + commits[parentIdx].lane * laneWidth, (parentIdx * 60) + 30);
        ctx.stroke();
      });

      // 文字
      ctx.fillStyle = '#fff';
      ctx.fillText(commit.message.slice(0, 40), 120, y + 5);
    });
  }
</script>

<canvas
  bind:this={canvas}
  width="1200"
  height="800"
  class="max-w-full h-auto rounded-lg ring-1 ring-white/[0.06]"
  style="background: var(--wf-term-bg);"
></canvas>
<button
  type="button"
  class="mt-3 w-full rounded-xl px-3 py-2 text-[13px] font-medium text-[var(--wf-fg)] bg-[var(--wf-surface)] border border-[var(--wf-border)] hover:border-violet-400/25 hover:bg-violet-500/[0.06] transition-colors"
  onclick={() => loadGraph()}
>
  刷新 Git Graph
</button>
{#if commits.length === 0}
  <p class="mt-3 text-[11px] leading-relaxed text-[var(--wf-fg-muted)]">
    后端尚未返回提交数据（当前为空占位，待 git2 接入）。
  </p>
{/if}