<script lang="ts">
  import { X } from 'lucide-svelte';
  import MainApp from '../../../remote/MainApp.svelte';
  import {
    activeSharedWorkspaceProjection,
    closeSharedWorkspaceProjection,
  } from '$lib/remote/cloud/sharedWorkspaceProjection';
</script>

{#if $activeSharedWorkspaceProjection}
  {@const projection = $activeSharedWorkspaceProjection}
  <section class="absolute inset-0 z-20 flex min-h-0 flex-col bg-[var(--rg-bg)]">
    <header class="flex h-9 shrink-0 items-center gap-2 border-b border-[var(--rg-border)] px-3">
      <span class="min-w-0 flex-1 truncate text-[11px] text-[var(--rg-fg-muted)]">
        共享：{projection.name} · {projection.ownerUsername}/{projection.deviceName}
      </span>
      <span class="rounded border border-[var(--rg-border)] px-1.5 text-[9px] text-[var(--rg-fg-muted)]">
        operator · 禁止二次转发
      </span>
      <button
        type="button"
        title="关闭共享工作区视图"
        class="flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-fg)]"
        onclick={closeSharedWorkspaceProjection}
      >
        <X class="h-3.5 w-3.5" />
      </button>
    </header>
    <div class="relative min-h-0 flex-1">
      <MainApp
        ws={projection.link}
        dataProvider={projection.dataProvider}
        workspaceManagement={false}
        embedded
      />
    </div>
  </section>
{/if}
