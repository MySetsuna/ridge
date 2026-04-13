<script lang="ts">
  import { onMount } from 'svelte';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import {
    projectStore,
    fileTree,
    recentProjects,
    isLoading,
    error,
    openProject,
    refreshFileTree,
    getDirectoryChildren,
    refreshRecentProjects,
    removeProject,
    initializeProjectStore,
    type FileNode,
  } from '$lib/stores/project';

  let expandedNodes = new Set<string>();

  onMount(() => {
    initializeProjectStore();
  });

  async function handleOpenFolder() {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: 'Open Project Folder',
    });

    if (selected) {
      await openProject(selected as string);
    }
  }

  async function toggleNode(node: FileNode) {
    if (!node.is_dir) return;

    if (expandedNodes.has(node.path)) {
      expandedNodes.delete(node.path);
    } else {
      // Load children if not loaded
      if (!node.children || node.children.length === 0) {
        const children = await getDirectoryChildren(node.path);
        node.children = children;
      }
      expandedNodes.add(node.path);
    }

    // Trigger reactivity
    expandedNodes = expandedNodes;
    projectStore.update(s => ({ ...s, fileTree: $fileTree }));
  }

  function isExpanded(path: string): boolean {
    return expandedNodes.has(path);
  }

  async function openRecentProject(path: string) {
    await openProject(path);
  }

  async function handleRemoveProject(projectId: number, event: Event) {
    event.stopPropagation();
    if (confirm('Remove this project from recent list?')) {
      await removeProject(projectId);
    }
  }

  function getFileIcon(node: FileNode): string {
    if (node.is_dir) {
      return expandedNodes.has(node.path) ? '📂' : '📁';
    }

    const ext = node.name.split('.').pop()?.toLowerCase();
    const icons: Record<string, string> = {
      ts: '🔷',
      tsx: '⚛️',
      js: '🟨',
      jsx: '⚛️',
      svelte: '🧡',
      rs: '🦀',
      py: '🐍',
      go: '🐹',
      md: '📝',
      json: '📋',
      toml: '⚙️',
      html: '🌐',
      css: '🎨',
      scss: '🎨',
    };

    return icons[ext || ''] || '📄';
  }
</script>

<div class="project-sidebar">
  <div class="sidebar-header">
    <h3>Explorer</h3>
    <button class="icon-btn" on:click={handleOpenFolder} title="Open Folder">
      📁
    </button>
  </div>

  {#if $isLoading}
    <div class="loading">Loading...</div>
  {:else if $error}
    <div class="error">{$error}</div>
  {:else if $fileTree}
    <div class="file-tree">
      {#each $fileTree.children || [$fileTree] as node}
        <svelte:self {node} {expandedNodes} on:toggle={(e) => toggleNode(e.detail)} depth={0} />
      {/each}
    </div>
  {:else}
    <div class="no-project">
      <p>No project open</p>
      <button class="open-btn" on:click={handleOpenFolder}>
        Open Folder
      </button>
    </div>
  {/if}

  {#if $recentProjects.length > 0 && !$fileTree}
    <div class="recent-projects">
      <h4>Recent Projects</h4>
      {#each $recentProjects as project}
        <div
          class="recent-item"
          on:click={() => openRecentProject(project.path)}
          on:keydown={(e) => e.key === 'Enter' && openRecentProject(project.path)}
          role="button"
          tabindex="0"
        >
          <span class="project-name">{project.name}</span>
          <span class="project-path">{project.path}</span>
          <button
            class="remove-btn"
            on:click={(e) => handleRemoveProject(project.id, e)}
            title="Remove"
          >
            ×
          </button>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if $$props.node}
  {@const node = $$props.node as FileNode}
  {@const depth = ($$props.depth as number) || 0}
  <div
    class="tree-node"
    class:directory={node.is_dir}
    style="padding-left: {depth * 16 + 8}px"
    on:click={() => toggleNode(node)}
    on:keydown={(e) => e.key === 'Enter' && toggleNode(node)}
    role="button"
    tabindex="0"
  >
    <span class="icon">{getFileIcon(node)}</span>
    <span class="name">{node.name}</span>
  </div>

  {#if node.is_dir && node.children && expandedNodes.has(node.path)}
    {#each node.children as child}
      <svelte:self node={child} {expandedNodes} depth={depth + 1} />
    {/each}
  {/if}
{/if}

<style>
  .project-sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #1e1e1e;
    color: #ccc;
    font-size: 13px;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid #333;
  }

  .sidebar-header h3 {
    margin: 0;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: #aaa;
  }

  .icon-btn {
    background: none;
    border: none;
    cursor: pointer;
    padding: 2px 6px;
    font-size: 14px;
    color: #ccc;
    border-radius: 3px;
  }

  .icon-btn:hover {
    background: #333;
  }

  .loading,
  .error {
    padding: 12px;
    text-align: center;
  }

  .error {
    color: #f48771;
  }

  .no-project {
    padding: 20px;
    text-align: center;
  }

  .no-project p {
    margin-bottom: 12px;
    color: #888;
  }

  .open-btn {
    background: #0e639c;
    color: white;
    border: none;
    padding: 6px 12px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
  }

  .open-btn:hover {
    background: #1177bb;
  }

  .file-tree {
    flex: 1;
    overflow: auto;
    padding: 4px 0;
  }

  .tree-node {
    display: flex;
    align-items: center;
    padding: 3px 8px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tree-node:hover {
    background: #2a2d2e;
  }

  .tree-node .icon {
    margin-right: 6px;
    font-size: 12px;
  }

  .tree-node .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .recent-projects {
    border-top: 1px solid #333;
    padding: 8px 0;
  }

  .recent-projects h4 {
    margin: 0;
    padding: 8px 12px;
    font-size: 11px;
    text-transform: uppercase;
    color: #aaa;
  }

  .recent-item {
    display: flex;
    flex-direction: column;
    padding: 8px 12px;
    cursor: pointer;
    position: relative;
  }

  .recent-item:hover {
    background: #2a2d2e;
  }

  .recent-item .project-name {
    font-weight: 500;
    margin-bottom: 2px;
  }

  .recent-item .project-path {
    font-size: 11px;
    color: #666;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .recent-item .remove-btn {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    color: #666;
    cursor: pointer;
    font-size: 16px;
    padding: 2px 6px;
    opacity: 0;
    transition: opacity 0.2s;
  }

  .recent-item:hover .remove-btn {
    opacity: 1;
  }

  .recent-item .remove-btn:hover {
    color: #f48771;
  }
</style>