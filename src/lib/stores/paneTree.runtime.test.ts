import { describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
const mocks=vi.hoisted(()=>({invoke:vi.fn()}));
vi.mock('@tauri-apps/api/core',()=>({invoke:mocks.invoke,isTauri:()=>true}));
vi.mock('@tauri-apps/api/event',()=>({listen:vi.fn(async()=>vi.fn())}));
vi.mock('@ridge/remote/shared/terminal/manager',()=>({TerminalManager:{instance:()=>({fitPaneNow:vi.fn(),detach:vi.fn(),forceFullRedrawFor:vi.fn()})}}));
vi.mock('@ridge/remote/shared/terminal/ptyBridge',()=>({ensurePtyBridge:vi.fn(async()=>{}),teardownPtyBridge:vi.fn()}));
describe('native Runtime workspace routing',()=>{
  it('refreshes and switches through the per-client service selection',async()=>{
    const client=await import('./paneTree');
    const views=[{id:'runtime-view',index:0,displaySeq:1},{id:'other-view',index:1,displaySeq:2}];
    mocks.invoke.mockImplementation(async (command:string,args?:{workspaceId?:string})=>{
      if(command==='list_workspaces') return views;
      if(command==='get_active_workspace_id') return 'runtime-view';
      if(command==='create_workspace') {views.push({id:'created-view',index:2,displaySeq:3});return 'created-view';}
      if(command==='get_pane_layout_for'||command==='get_pane_layout') return {type:'leaf',id:`pane-${args?.workspaceId ?? 'runtime-view'}`};
      if(command==='list_workspace_save_info') return [];
      return undefined;
    });
    await client.refreshWorkspaces();
    expect(get(client.activeWorkspaceId)).toBe('runtime-view');
    expect(mocks.invoke).toHaveBeenCalledWith('get_active_workspace_id');
    expect(mocks.invoke).not.toHaveBeenCalledWith('acquire_window_workspace');
    expect(await client.switchWorkspace('other-view')).toBe(true);
    expect(get(client.activeWorkspaceId)).toBe('other-view');
    expect(mocks.invoke).toHaveBeenCalledWith('switch_workspace',{workspaceId:'other-view'});
    expect(mocks.invoke.mock.calls.some(([method])=>method==='claim_workspace_window'||method==='switch_window_workspace')).toBe(false);
    await client.createWorkspace();
    expect(get(client.activeWorkspaceId)).toBe('created-view');
    expect(get(client.workspacesList).some(view=>view.id==='created-view')).toBe(true);
    expect(mocks.invoke).toHaveBeenCalledWith('create_workspace');
    expect(mocks.invoke).toHaveBeenCalledWith('switch_workspace', { workspaceId: 'created-view' });
    expect(mocks.invoke.mock.calls.some(([method])=>method==='create_workspace_for_window')).toBe(false);

  });
});
