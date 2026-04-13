<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import {
    projectStore,
    searchResults,
    isSearching,
    textSearch,
    replaceInFiles,
    filenameSearch,
    clearSearch,
    type SearchResult,
  } from '$lib/stores/project';

  const dispatch = createEventDispatcher();

  let searchQuery = '';
  let replaceQuery = '';
  let caseSensitive = false;
  let useRegex = false;
  let wholeWord = false;
  let mode: 'search' | 'replace' = 'search';
  let searchType: 'text' | 'filename' = 'text';
  let filenameResults: string[] = [];
  let filenameQuery = '';

  async function handleSearch() {
    if (!searchQuery.trim()) return;

    if (searchType === 'text') {
      await textSearch(searchQuery, {
        caseSensitive,
        useRegex,
        wholeWord,
      });
    } else {
      filenameResults = await filenameSearch(searchQuery);
    }
  }

  async function handleReplace() {
    if (!searchQuery.trim() || !replaceQuery.trim()) return;

    const files = [...new Set($searchResults.map(r => r.file))];
    const stats = await replaceInFiles(searchQuery, replaceQuery, files, {
      caseSensitive,
      useRegex,
    });

    alert(`Replaced in ${stats.files_modified} files (${stats.replacements} replacements)`);

    if (stats.files_modified > 0) {
      // Refresh search results
      await handleSearch();
    }
  }

  function close() {
    clearSearch();
    dispatch('close');
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      close();
    } else if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      if (mode === 'search') {
        handleSearch();
      } else {
        handleReplace();
      }
    }
  }

  function handleFilenameSelect(path: string) {
    dispatch('openFile', { path });
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

<div class="search-modal-overlay" on:click={close} on:keydown={() => {}} role="button" tabindex="-1">
  <div class="search-modal" on:click|stopPropagation on:keydown={() => {}} role="dialog" tabindex="0">
    <div class="search-header">
      <div class="search-tabs">
        <button
          class="tab"
          class:active={searchType === 'text'}
          on:click={() => (searchType = 'text')}
        >
          Find in Files
        </button>
        <button
          class="tab"
          class:active={searchType === 'filename'}
          on:click={() => (searchType = 'filename')}
        >
          File Name
        </button>
      </div>

      {#if searchType === 'text'}
        <div class="mode-toggle">
          <button
            class="mode-btn"
            class:active={mode === 'search'}
            on:click={() => (mode = 'search')}
          >
            Search
          </button>
          <button
            class="mode-btn"
            class:active={mode === 'replace'}
            on:click={() => (mode = 'replace')}
          >
            Replace
          </button>
        </div>
      {/if}

      <button class="close-btn" on:click={close}>×</button>
    </div>

    <div class="search-inputs">
      {#if searchType === 'text'}
        <input
          type="text"
          bind:value={searchQuery}
          placeholder="Search..."
          class="search-input"
        />

        {#if mode === 'replace'}
          <input
            type="text"
            bind:value={replaceQuery}
            placeholder="Replace with..."
            class="replace-input"
          />
        {/if}

        <div class="search-options">
          <label>
            <input type="checkbox" bind:checked={caseSensitive} />
            Match Case
          </label>
          <label>
            <input type="checkbox" bind:checked={useRegex} />
            Regex
          </label>
          <label>
            <input type="checkbox" bind:checked={wholeWord} />
            Whole Word
          </label>
        </div>

        <button class="search-btn" on:click={handleSearch} disabled={$isSearching}>
          {$isSearching ? 'Searching...' : 'Search'}
        </button>

        {#if mode === 'replace'}
          <button class="replace-btn" on:click={handleReplace} disabled={$searchResults.length === 0}>
            Replace All
          </button>
        {/if}
      {:else}
        <input
          type="text"
          bind:value={searchQuery}
          placeholder="Type to search files..."
          class="search-input"
          on:input={handleSearch}
        />
      {/if}
    </div>

    <div class="search-results">
      {#if $isSearching}
        <div class="searching">Searching...</div>
      {:else if searchType === 'text' && $searchResults.length > 0}
        <div class="results-count">{$searchResults.length} results</div>
        <div class="results-list">
          {#each $searchResults as result}
            <div
              class="result-item"
              on:click={() => dispatch('openFile', { path: result.file, line: result.line })}
              on:keydown={(e) => e.key === 'Enter' && dispatch('openFile', { path: result.file, line: result.line })}
              role="button"
              tabindex="0"
            >
              <div class="result-file">
                <span class="file-name">{getFileName(result.file)}</span>
                <span class="file-line">:{result.line}</span>
              </div>
              <div class="result-content">{result.content}</div>
            </div>
          {/each}
        </div>
      {:else if searchType === 'filename' && filenameResults.length > 0}
        <div class="results-count">{filenameResults.length} files</div>
        <div class="results-list">
          {#each filenameResults as path}
            <div
              class="result-item"
              on:click={() => handleFilenameSelect(path)}
              on:keydown={(e) => e.key === 'Enter' && handleFilenameSelect(path)}
              role="button"
              tabindex="0"
            >
              <div class="result-file">
                <span class="file-name">{getFileName(path)}</span>
              </div>
              <div class="result-dir">{getDirectory(path)}</div>
            </div>
          {/each}
        </div>
      {:else if searchQuery && !$isSearching}
        <div class="no-results">No results found</div>
      {/if}
    </div>
  </div>
</div>

<style>
  .search-modal-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    justify-content: center;
    padding-top: 60px;
    z-index: 1000;
  }

  .search-modal {
    background: #252526;
    border: 1px solid #3c3c3c;
    border-radius: 6px;
    width: 700px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .search-header {
    display: flex;
    align-items: center;
    padding: 8px 12px;
    border-bottom: 1px solid #3c3c3c;
    gap: 8px;
  }

  .search-tabs {
    display: flex;
    gap: 4px;
    flex: 1;
  }

  .tab {
    background: none;
    border: none;
    color: #969696;
    padding: 6px 12px;
    cursor: pointer;
    border-radius: 3px;
    font-size: 13px;
  }

  .tab.active {
    background: #37373d;
    color: #fff;
  }

  .mode-toggle {
    display: flex;
    background: #2d2d30;
    border-radius: 3px;
    padding: 2px;
  }

  .mode-btn {
    background: none;
    border: none;
    color: #969696;
    padding: 4px 10px;
    cursor: pointer;
    border-radius: 2px;
    font-size: 12px;
  }

  .mode-btn.active {
    background: #37373d;
    color: #fff;
  }

  .close-btn {
    background: none;
    border: none;
    color: #969696;
    font-size: 20px;
    cursor: pointer;
    padding: 0 4px;
  }

  .close-btn:hover {
    color: #fff;
  }

  .search-inputs {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .search-input,
  .replace-input {
    background: #3c3c3c;
    border: 1px solid #3c3c3c;
    color: #ccc;
    padding: 8px 10px;
    border-radius: 3px;
    font-size: 13px;
    width: 100%;
    box-sizing: border-box;
  }

  .search-input:focus,
  .replace-input:focus {
    outline: none;
    border-color: #007acc;
  }

  .search-options {
    display: flex;
    gap: 16px;
  }

  .search-options label {
    display: flex;
    align-items: center;
    gap: 4px;
    color: #969696;
    font-size: 12px;
    cursor: pointer;
  }

  .search-options input[type="checkbox"] {
    cursor: pointer;
  }

  .search-btn,
  .replace-btn {
    background: #0e639c;
    color: white;
    border: none;
    padding: 8px 16px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 13px;
  }

  .search-btn:hover,
  .replace-btn:hover {
    background: #1177bb;
  }

  .search-btn:disabled,
  .replace-btn:disabled {
    background: #3c3c3c;
    cursor: not-allowed;
  }

  .replace-btn {
    background: #0e639c;
  }

  .search-results {
    flex: 1;
    overflow: auto;
    border-top: 1px solid #3c3c3c;
  }

  .results-count {
    padding: 8px 12px;
    font-size: 12px;
    color: #969696;
    border-bottom: 1px solid #3c3c3c;
  }

  .results-list {
    max-height: 400px;
    overflow: auto;
  }

  .result-item {
    padding: 8px 12px;
    cursor: pointer;
    border-bottom: 1px solid #2d2d30;
  }

  .result-item:hover {
    background: #2a2d2e;
  }

  .result-file {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 4px;
  }

  .file-name {
    color: #9cdcfe;
    font-size: 13px;
  }

  .file-line {
    color: #858585;
    font-size: 12px;
  }

  .result-content {
    font-size: 12px;
    color: #ce9178;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: monospace;
  }

  .result-dir {
    font-size: 12px;
    color: #858585;
  }

  .searching,
  .no-results {
    padding: 40px;
    text-align: center;
    color: #969696;
  }
</style>