import { describe, expect, it } from 'vitest';
import { readTerminalLifecycle, terminalLifecycleMessage } from './lifecycle';
const pane = {workspaceId:'workspace',paneId:'pane'};
const live = {...pane,runtimeEpoch:'runtime',terminalEpoch:'second',ptyGeneration:2,status:'Idle',error:null};
describe('Runtime terminal lifecycle projection',()=>{
  it('rejects another pane, old activation, old child and contradictory lifetime',()=>{
    const current=readTerminalLifecycle(live,pane,null,7)!;
    for(const changed of [{paneId:'other'},{workspaceId:'other'},{activationId:6},{ptyGeneration:1,terminalEpoch:'first',status:'Exited'},{terminalEpoch:'contradiction'}]) {
      expect(readTerminalLifecycle({...live,...changed},pane,current,7)).toBeNull();
    }
    expect(readTerminalLifecycle({...live,activationId:7},pane,current,7)).toEqual(current);
    expect(readTerminalLifecycle({...live,runtimeEpoch:'restarted',ptyGeneration:0,terminalEpoch:'new'},pane,current,8)?.terminalEpoch).toBe('new');
  });
  it('shows actual stopped/failure state without inventing a restart',()=>{
    const stopped=readTerminalLifecycle({...live,status:'Exited'},pane,null)!;
    expect(terminalLifecycleMessage(stopped)).toContain('Terminal exited');
    expect(terminalLifecycleMessage({...stopped,error:'outside Runtime roots'})).toContain('Shell could not restart');
    expect(terminalLifecycleMessage(readTerminalLifecycle(live,pane,stopped))).toBeNull();
    for(const payload of [null,{}, {...live,ptyGeneration:-1},{...live,ptyGeneration:NaN},{...live,status:'guessed busy'}]) expect(readTerminalLifecycle(payload,pane,null)).toBeNull();
  });
});
