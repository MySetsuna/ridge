<script lang="ts">
  import type { ProviderInput } from './agentEvents';
  export let provider: ProviderInput = { kind: 'mock', model: { id: 'mock-claude-haiku', tier: 'base', effort: 'medium' }, config: {} };
  export let disabled = false;
  export let onNewChat: (() => void | Promise<void>) | undefined = undefined;
</script>
<div class="picker">
  <label>Provider <select bind:value={provider.kind} disabled={disabled}>
    <option value="mock">Mock fixture</option><option value="opencode_acp">OpenCode ACP</option><option value="open_ai">OpenAI via ACP</option><option value="anthropic">Anthropic via ACP</option>
  </select></label>
  <label>Model <input aria-label="Model" bind:value={provider.model.id} disabled={disabled} /></label>
  <label>Effort <select bind:value={provider.model.effort} disabled={disabled}><option value="low">Low</option><option value="medium">Medium</option><option value="high">High</option></select></label>
  <button type="button" on:click={() => onNewChat?.()} disabled={disabled}>New Chat</button>
</div>
<style>
.picker { display:flex; gap:6px; align-items:end; flex-wrap:wrap; font-size:11px; }
label { display:flex; flex-direction:column; gap:3px; min-width:0; flex:1; }
input, select { min-width:0; width:100%; background:var(--rg-bg); color:var(--rg-fg); border:1px solid var(--rg-border); padding:4px; }
button { padding:5px; }
</style>
