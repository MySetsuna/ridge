/**
 * 扩展命名空间汇总。每个 *.ts 模块导出 { zh, en } 两个子字典，
 * 在此按 namespace 键合并进主目录，供 $t('<ns>.<key>') 使用。
 *
 * 分模块设计：不同界面域各占一个文件，便于并行扩展、零写冲突。
 */
import * as explorer from './explorer';
import * as scm from './scm';
import * as editor from './editor';
import * as workspace from './workspace';
import * as ui from './ui';
import * as mobile from './mobile';
import * as main from './main';
import * as commune from './commune';

type Dict = Record<string, unknown>;

export const extraZh: Dict = {
  explorer: explorer.zh,
  scm: scm.zh,
  editor: editor.zh,
  workspace: workspace.zh,
  ui: ui.zh,
  mobile: mobile.zh,
  main: main.zh,
  commune: commune.zh
};

export const extraEn: Dict = {
  explorer: explorer.en,
  scm: scm.en,
  editor: editor.en,
  workspace: workspace.en,
  ui: ui.en,
  mobile: mobile.en,
  main: main.en,
  commune: commune.en
};
