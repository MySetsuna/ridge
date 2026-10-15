import {afterEach,beforeEach,describe,expect,it,vi} from 'vitest';
import {get} from 'svelte/store';
const mocks=vi.hoisted(()=>({invoke:vi.fn(),snapshot:vi.fn()}));
vi.mock('@tauri-apps/api/core',()=>({invoke:mocks.invoke}));
vi.mock('$lib/i18n',()=>({tr:(key:string)=>key}));
vi.mock('@ridge/remote/shared/cloud/auth',()=>({snapshot:mocks.snapshot}));
vi.mock('@ridge/remote/shared/cloud/apiClient',()=>({BASE_DOMAIN:'owned.example',cloudHttpScheme:()=> 'https'}));
import {hostState,hostError,cloudSessions,goOnline,goOffline,refreshCloudHost,startCloudHostProjection,kickController,blacklistController} from './cloudHostStore';
let stop:(()=>void)|undefined;
let runtime:{state:string,error:string,sessions:{cid:string,state:string,connectedAt:number}[]};
describe('Runtime Cloud projection',()=>{
  beforeEach(()=>{
    runtime={state:'offline',error:'',sessions:[]};
    mocks.invoke.mockReset();mocks.snapshot.mockReset();mocks.snapshot.mockReturnValue({});
    mocks.invoke.mockImplementation(async(command:string)=>command==='get_cloud_host_state'?runtime:undefined);
    hostState.set('offline');hostError.set('');cloudSessions.set([]);
  });
  afterEach(()=>{stop?.();stop=undefined;vi.useRealTimers();});
  it('rejects missing activation credentials without requesting a host',async()=>{
    await goOnline();expect(mocks.invoke).not.toHaveBeenCalled();expect(get(hostError)).toBe('cloud.errDeviceNotActivated');
  });
  it('passes credentials only to Runtime and displays its confirmed state',async()=>{
    mocks.snapshot.mockReturnValue({deviceToken:'owned-device',deviceName:'desktop',user:{username:'owner'}});
    runtime={state:'connecting',error:'',sessions:[]};
    await goOnline();
    expect(mocks.invoke).toHaveBeenNthCalledWith(1,'cloud_host_online',{deviceToken:'owned-device',deviceName:'desktop',username:'owner',baseDomain:'owned.example',plaintext:false});
    expect(mocks.invoke).toHaveBeenNthCalledWith(2,'get_cloud_host_state');expect(get(hostState)).toBe('connecting');
  });
  it('reattaches an existing service projection and never stops it on window detach',async()=>{
    vi.useFakeTimers();runtime={state:'online',error:'',sessions:[{cid:'existing',state:'connected',connectedAt:100}]};
    stop=startCloudHostProjection();await refreshCloudHost();
    expect(get(cloudSessions)[0].cid).toBe('existing');expect(get(hostState)).toBe('online');
    await vi.advanceTimersByTimeAsync(2000);stop();stop=undefined;
    const calls=mocks.invoke.mock.calls.length;await vi.advanceTimersByTimeAsync(4000);expect(mocks.invoke.mock.calls).toHaveLength(calls);
    expect(mocks.invoke.mock.calls.every(([method])=>method==='get_cloud_host_state')).toBe(true);
  });
  it('keeps the last confirmed state and reports a failed control action',async()=>{
    hostState.set('online');mocks.invoke.mockRejectedValueOnce(new Error('Runtime unavailable'));
    await goOffline();expect(get(hostState)).toBe('online');expect(get(hostError)).toBe('Runtime unavailable');
  });
  it('reports query failure without replacing existing sessions with an empty snapshot',async()=>{
    cloudSessions.set([{cid:'existing',state:'connected',connectedAt:100}]);mocks.invoke.mockRejectedValueOnce(new Error('query failed'));
    await refreshCloudHost();expect(get(cloudSessions)).toHaveLength(1);expect(get(hostError)).toBe('query failed');
  });
  it('sends kick and blacklist actions to the same service',async()=>{
    kickController('one');await vi.waitFor(()=>expect(mocks.invoke).toHaveBeenCalledWith('cloud_host_kick',{cid:'one'}));
    await refreshCloudHost();blacklistController('two');await vi.waitFor(()=>expect(mocks.invoke).toHaveBeenCalledWith('cloud_host_blacklist',{cid:'two'}));await refreshCloudHost();
  });
});
