<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { filenameSearch } from '$lib/stores/project';

  const dispatch = createEventDispatcher();

  let query = '';
  let results: string[] = [];
  let selectedIndex = 0;
  let inputEl: HTMLInputElement;
  let isLoading = false;

  onMount(() => {
    inputEl?.focus();
  });

  async function handleInput() {
    if (!query.trim()) {
      results = [];
      return;
    }

    isLoading = true;
    results = await filenameSearch(query);
    selectedIndex = 0;
    isLoading = false;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      dispatch('close');
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, results.length - 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
    } else if (event.key === 'Enter' && results[selectedIndex]) {
      selectFile(results[selectedIndex]);
    }
  }

  function selectFile(path: string) {
    dispatch('openFile', { path });
    dispatch('close');
  }

  function getFileName(path: string): string {
    return path.split(/[/\\]/).pop() || path;
  }

  function getDirectory(path: string): string {
    const parts = path.split(/[/\\]/);
    parts.pop();
    return parts.join('/');
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="quick-open-overlay" on:click={() => dispatch('close')} on:keydown={() => {}} role="button" tabindex="-1">
  <div class="quick-open" on:click|stopPropagation on:keydown={() => {}} role="dialog" tabindex="0">
    <div class="search-box">
      <input
        bind:this={inputEl}
        type="text"
        bind:value={query}
        on:input={handleInput}
        placeholder="Search files..."
        class="search-input"
      />
    </div>

    <div class="results">
      {#if isLoading}
        <div class="loading">Searching...</div>
      {:else if results.length > 0}
        {#each results as path, index}
          <div
            class="result-item"
            class:selected={index === selectedIndex}
            on:click={() => selectFile(path)}
            on:mouseenter={() => (selectedIndex = index)}
            on:keydown={(e) => e.key === 'Enter' && selectFile(path)}
            role="option"
            tabindex="0"
            aria-selected={index === selectedIndex}
          >
            <span class="file-icon">📄</span>
            <div class="file-info">
              <span class="file-name">{getFileName(path)}</span>
              <span class="file-dir">{getDirectory(path)}</span>
            </div>
          </div>
        {/each}
      {:else if query}
        <div class="no-results">No files found</div>
      {:else}
        <div class="hint">Start typing to search files...</div>
      {/if}
    </div>

    <div class="footer">
      <span class="hint-text">
        <kbd>↑↓</kbd> navigate
        <kbd>Enter</kbd> open
        <kbd>Esc</kbd> close
      </span>
    </div>
  </div>
</div>

<style>
  .quick-open-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    justify-content: center;
    padding-top: 100px;
    /* z-index: 1000; */
  }

  .quick-open {
    background: #252526;
    border: 1px solid #3c3c3c;
    border-radius: 6px;
    width: 600px;
    max-height: 500px;
    display: flex;
    flex-direction: column;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }

  .search-box {
    padding: 12px;
    border-bottom: 1px solid #3c3c3c;
  }

  .search-input {
    background: #3c3c3c;
    border: 1px solid #3c3c3c;
    color: #ccc;
    padding: 10px 12px;
    border-radius: 3px;
    font-size: 14px;
    width: 100%;
    box-sizing: border-box;
  }

  .search-input:focus {
    outline: none;
    border-color: #007acc;
  }

  .results {
    flex: 1;
    overflow: auto;
    max-height: 350px;
  }

  .result-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    cursor: pointer;
  }

  .result-item:hover,
  .result-item.selected {
    background: #094771;
  }

  .file-icon {
    font-size: 16px;
  }

  .file-info {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .file-name {
    color: #d4d4d4;
    font-size: 14px;
  }

  .file-dir {
    color: #858585;
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .loading,
  .no-results,
  .hint {
    padding: 20px;
    text-align: center;
    color: #969696;
    font-size: 13px;
  }

  .footer {
    padding: 8px 12px;
    border-top: 1px solid #3c3c3c;
    background: #2d2d30;
  }

  .hint-text {
    font-size: 11px;
    color: #858585;
  }

  kbd {
    background: #3c3c3c;
    padding: 2px 5px;
    border-radius: 3px;
    font-size: 11px;
    margin-right: 4px;
  }
</style>