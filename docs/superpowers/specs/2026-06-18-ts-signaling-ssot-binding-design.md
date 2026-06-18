# TS 侧接入信令 SSOT(生成闭环)设计

> 日期:2026-06-18 ｜ 仓库:`C:\code\wind`(ridge 桌面 host + controller TS)
> 关联:`ridge-signaling`(SSOT,rev `7f958215`)、`.agent-team/findings-align.md` P1-1
> 状态:设计已审批,待落实现计划

## 背景与动机

远控信令协议的单一事实来源(SSOT)已抽成独立 crate `ridge-signaling`(`SignalMsg`/`Role`/
错误码 + ts-rs 生成的 TS bindings + golden fixtures + Rust 侧跨语言 conformance)。**Rust 两端
已真正对齐**:

- `ridge-cloud/src/ws/messages.rs` → `pub use ridge_signaling::SignalMsg as ServerEvent`,git rev 锁 `7f958215`。
- `wind/packages/ridge-cli/src/signaling.rs` → `pub use ridge_signaling::{Role, SignalMsg}`,同一 rev;P0(cli 缺 cid)运行时已闭合(`session.rs` 入站 offer 捕获 cid、出站 answer/ice 回盖)。

**残留缺口(本设计要关掉的)**:wind 内**不存在** `SignalMsg.ts`——两个 TS provider
(`ridgeCloudProvider.ts` host / `controllerCloudProvider.ts` controller)各自**手写** `SignalIn`
类型,未 import 生成的 bindings,且无 TS 侧 fixtures conformance 兜底。线形目前一致(host 带
`cid`、controller 故意省 `cid`,审计标注为"正确"),但属"人工镜像"而非"生成闭环":日后改
`lib.rs` 后,TS 手写副本不会被任何测试拦住漂移。findings-align P1-1 建议的"TS 侧由其生成/对照"
只完成了"对照"半步。

## 目标 / 非目标

**目标**

- 把生成的 `SignalMsg.ts`/`Role.ts` + golden fixtures 引入 wind,作为 TS 侧唯一类型来源。
- 两个 provider 删除手写 `SignalIn`,改用生成类型(`role` 字面量 → `Role`)。
- 新增 TS 侧读 fixtures 的跨语言 conformance,把线形(camelCase / cid 取舍 / kebab tag / ice null /
  未知 tag 前向兼容)钉死。
- 新增"在场即比对"的漂移守卫,使 ridge-signaling 重生成而 wind 忘同步时测试立刻红。

**非目标(YAGNI)**

- 不动 Rust 侧、不动线协议字节、不改 provider 运行时行为(纯类型来源切换)。
- 不给 ridge-signaling 加 npm 包基建。
- 不实现 P1-2 多控制方(ridge-cli 单会话是独立取舍,不在本次)。
- 不强行从联合类型外科手术去掉 controller 的可选 cid(见 §决策 A)。

## 方案选型(已审批)

**bindings/fixtures 进 wind 的方式** → **vendor 拷贝 + 同步脚本 + 漂移守卫**(已选)。
- 备选「测试期读同级路径」:CI 上同级未必 checkout → 测试挂;跨仓 import 越过 tsconfig rootDir。否决。
- 备选「ridge-signaling 发 npm 包」:需新增 JS 包基建、pnpm git-subpath 依赖偏重,与现状"committed .ts
  bindings"不符。否决。
- 选定理由:CI 零依赖(不需同级 checkout),与 Rust 侧"锁 rev"哲学一致(此处以"提交 vendored 副本 +
  记录 rev"等价锁定)。

## 详细设计

### 1. 目录与 vendor 落点

wind 新增 `src/lib/remote/cloud/signaling/`:

```
signaling/
├── generated/                 # 只读·机器生成,禁止手改(文件头保留 ts-rs "Do not edit" 注释)
│   ├── SignalMsg.ts           # ← ridge-signaling/bindings/SignalMsg.ts 拷贝
│   ├── Role.ts                # ← ridge-signaling/bindings/Role.ts
│   └── serde_json/
│       └── JsonValue.ts       # ← ridge-signaling/bindings/serde_json/JsonValue.ts
├── fixtures/                  # ← ridge-signaling/fixtures/signaling/*.json 全量拷贝(16 个)
├── index.ts                   # 薄 re-export + 收窄子类型 + parseSignal()
├── SOURCE_REV                 # 单行:来源 ridge-signaling commit(初始 = 7f958215...)
├── conformance.test.ts        # 读 fixtures 的跨语言对照
└── drift.test.ts              # 在场即比对的漂移守卫
```

fixtures 全量(16):`answer_cid` `e2ee_pubkey_cid` `e2ee_pubkey_no_cid` `error`
`ice_candidate_cid` `ice_null` `kick` `offer_cid` `offer_no_cid` `peer_join_controller`
`peer_join_host` `peer_leave_controller` `peer_leave_host` `unknown_forward_compat`
`welcome_controller` `welcome_host`。

### 2. 同步脚本

`scripts/sync-signaling.mjs`(Node ESM,与 wind 既有脚本风格一致):

- 解析同级 `../ridge-signaling` 绝对路径;不存在则报错退出并提示先 checkout。
- 拷贝 `bindings/{SignalMsg.ts,Role.ts,serde_json/JsonValue.ts}` → `generated/`。
- 拷贝 `fixtures/signaling/*.json` → `signaling/fixtures/`(先清空目标,避免源删文件后残留)。
- 读源 `git rev-parse HEAD` 写入 `SOURCE_REV`(单行,无换行噪声)。
- `package.json` `scripts` 加 `"sync:signaling": "node scripts/sync-signaling.mjs"`。

### 3. TS 消费形态(决策 A:薄 index.ts 单一消费点)

`signaling/index.ts` 作为 wind 侧唯一消费点(对应 Rust `messages.rs` 的 re-export):

```ts
export type { SignalMsg } from './generated/SignalMsg';
export type { Role } from './generated/Role';

// 两端入站子集:kick 是 host→relay 出站,任何一端都不会"收到"它。
export type SignalIn = Exclude<import('./generated/SignalMsg').SignalMsg, { t: 'kick' }>;

// 集中处理未知 tag 前向兼容(对应 unknown_forward_compat.json):
//  - 已知 tag → 收窄为 SignalMsg;未知 tag → 保留 { t } 由调用方忽略,绝不抛。
export function parseSignal(text: string): SignalMsg | { t: string };
```

两个 provider:
- 删除各自手写的 `type SignalIn = …`,改 `import type { SignalIn, Role } from '../signaling'`(host)
  / 对应相对路径(controller)。
- 收信号处 `JSON.parse` → 改走 `parseSignal`(统一未知 tag 兜底)。
- `role: 'host' | 'controller'` 内联字面量 → `Role`。
- 出站消息字面量(answer/ice/e2ee-pubkey/kick 等)以 `SignalMsg` 收窄校验。

**决策 A — controller 的 cid 取舍**:生成的 `SignalMsg` 每变体带 `cid?`。controller 改用后类型上
"看得见"可选 cid。审计强调 cid 由 relay 控、客户端不得据此做信任决策——但这是**运行时纪律**
(controller 代码本就不读 cid),类型保留可选 cid 无害;不为去掉它对联合类型做外科手术(过度设计)。
**runtime 行为零变更**。

### 4. conformance(决策 B)

`conformance.test.ts`(vitest,仿 `transport/remote/conformance.test.ts` 风格):

- 维护一张**强类型字面量表** `Record<fixtureName, SignalMsg>`(`unknown_forward_compat` 除外),
  逐条**双向对照**:
  - 解析向:`expect(JSON.parse(fixtureText)).toEqual(typedLiteral)` —— 钉死 `peerPresent` camelCase、
    `cid` 取舍、kebab tag(`peer-join`/`peer-leave`/`e2ee-pubkey`)、`ice` 的 `candidate:null`。
  - 序列化向:`expect(JSON.parse(JSON.stringify(typedLiteral))).toEqual(JSON.parse(fixtureText))`。
  - 字面量表类型标注为 `SignalMsg` → ridge-signaling 改 schema、重新 vendor 后:字段改名 → **编译
    报错**;值/形状变更 → **测试失败**。
- 覆盖断言:字面量表的 key 集合 === `fixtures/` 下文件名集合(减 `unknown_forward_compat`),防止
  新增 fixture 漏对照。
- `unknown_forward_compat.json` 单独断言:`parseSignal` 不抛、返回对象保留 `t`,且不被误判为已知变体。

### 5. 漂移守卫(决策 B 续)

`drift.test.ts`:

- 若同级 `../ridge-signaling` 在场(`fs.existsSync`):
  - 逐字节比对 wind vendored `generated/` == 源 `bindings/`、vendored `fixtures/` == 源
    `fixtures/signaling/`(含文件集合一致)。
  - 校验 `SOURCE_REV` 内容 === 源 `git rev-parse HEAD`。
  - 任一不符 → 失败,报错信息提示运行 `pnpm sync:signaling`。
- 同级缺席 → 整组 `it.skip`(CI 零依赖,仍跑 conformance against vendored fixtures)。

## 影响面与风险

- **改动文件**:新增 `signaling/` 整目录 + `scripts/sync-signaling.mjs` + `package.json` 一行;
  修改 `ridgeCloudProvider.ts`、`controllerCloudProvider.ts` 的类型来源与收信号入口。
- **安全敏感**:两个 provider 是远控安全路径。本次为**纯类型来源切换 + 收信号统一兜底**,不改密钥/
  绑定/鉴权逻辑;`parseSignal` 必须保持"未知 tag 不抛、不误判"以维持前向兼容(D-GM 既有行为)。
- **回归**:provider 既有单测(`ridgeCloudProvider.test.ts`/`controllerCloudProvider.test.ts`)需全绿;
  新增 conformance/drift 测试需全绿;`pnpm check`(svelte-check)类型零错。
- **风险点**:`SignalIn = Exclude<…,{t:'kick'}>` 之于 controller 仍含 `offer`/`answer`——controller
  实际只发 offer、收 answer,但收到 offer 也不应崩(忽略即可),与现状一致。

## 验收标准

1. `pnpm sync:signaling` 可从同级 ridge-signaling 幂等同步,`SOURCE_REV` 写入正确。
2. 两 provider 不再出现手写 `SignalIn`/`'host'|'controller'` 字面量,类型来自 `signaling/index.ts`。
3. `pnpm test` 下 conformance 16 条 fixtures 全覆盖且双向对照通过;drift 守卫在同级在场时通过、缺席时
   skip。
4. `pnpm check` 类型零错;provider 既有单测全绿。
5. 线协议字节零变更(由 conformance 双向对照间接证明 + 不改任何 `to_text`/序列化路径)。
