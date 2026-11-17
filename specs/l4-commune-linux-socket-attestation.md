---
id: L4-COMMUNE-LINUX-SOCKET-ATTESTATION
level: L4
title: Linux TCP 归属等待同一连接的可见证据，CI 严格传播失败
status: DRAFT
lifecycle: PLANNED
parent: L3-COMMUNE-PROTOCOL-MEMBERSHIP
code_targets:
  - packages/ridge-core/src/process_identity.rs
  - .github/workflows/rtp1-kernel-e2e.yml
test_targets:
  - packages/ridge-core/src/process_identity.rs
  - packages/ridge-kernel/tests/commune_registration.rs
  - .github/workflows/rtp1-kernel-e2e.yml
---

原始 CI 日志 43917900027 暴露 session choice 登记失败，但 cargo test 经 tee 的管道掩盖退出码。CI 统一使用严格 Bash/pipefail，并以有意失败的管道验证失败传播；真实回归任何失败均不得变绿。保留所有业务断言与已有用例，不降低认证或跳过 Commune 验收。

Linux 实测已 ESTABLISHED 的 TCP 在服务端 accept 前 inode 为 0、没有可归属 FD，accept 后才可见真实 inode。未知证据不能误称为另一个进程。沿现有最多 100ms 的同一 socket 观察周期等待可见的非零 inode；仍不能证明则拒绝，始终复核同一 PID 启动实例，不重连、不依赖端口或进程名。多个精确四元组记录、错误归属和 PID 复用仍拒绝。Windows 行为保持。

回归覆盖 Linux 零/缺失/歧义 inode、延迟 accept 的同一连接、错误进程与过期实例，以及真实 Kernel session/list 选择和 session/load 登记。平台能在 accept 前证明时保留该断言；Linux 必须有实际 accept 后的证据，不虚构尚不存在的进程归属。CI 实际执行 process_identity 回归族并检查原始日志，不以工作流绿色替代用例通过。
