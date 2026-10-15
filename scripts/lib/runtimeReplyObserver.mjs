// Test-only reference parser for the common PTY output observer; no product transport.
import {readFile} from 'node:fs/promises';
import init,{TerminalKernel} from '../../packages/ridge-term/pkg/ridge_term.js';

export async function createRuntimeReplyObserver(rows,cols){
 await init({module_or_path:await readFile(new URL('../../packages/ridge-term/pkg/ridge_term_bg.wasm',import.meta.url))});
 const terminal=new TerminalKernel(rows,cols,256);
 return {
  feed(bytes){terminal.feed(bytes);},
  hasReply(id){const found=terminal.searchSetQuery(`REPLY_${id}`,true)>0;terminal.scrollToBottom();return found;},
  cachedRows(){return terminal.scrollbackLen();},
  visibleText(){return terminal.dumpVisibleText();},
  close(){terminal.free();},
 };
}
