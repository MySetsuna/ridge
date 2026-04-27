<script lang="ts" module>
  // src/lib/components/DiffEditorModal.svelte
  //
  // Diff view is now integrated into FileEditor as a tab (no separate modal).
  // This module only exports the OpenDiffArgs type and openDiffEditor()
  // so existing callers need no changes.
  import { fileEditorStore } from '$lib/stores/fileEditor';

  export interface OpenDiffArgs {
    repoRoot: string;
    /** Repo-relative path. Used both as the tab title and as the git pathspec. */
    path: string;
    /**
     * `true` → staged view (HEAD vs index).
     * `false` → unstaged view (index vs working tree).
     */
    cached: boolean;
  }

  /** Open a diff tab inside the file editor. */
  export function openDiffEditor(args: OpenDiffArgs): void {
    fileEditorStore.openDiffTab(args);
  }
</script>
