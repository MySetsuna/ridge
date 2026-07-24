# Deep Research Report — Interaction / Multi-host / Collaboration / Fault Tolerance

- notebook: 66919cb9-1329-4ddf-955c-f426d15a9fe6
- task_id: 73ca5d9e-3ef6-4210-ab23-174c61fbdb73
- archived: 2026-07-24
- import policy: REPORT ONLY (no website sources)
- exemption: temporary second notebook source until all derived visions implemented

---

# Actionable Product Engineering Brief: Next-Generation Ridge Terminal 
Multi-Agent Control Plane

Operational complexity in multi-agent environments has evolved past simple 
text-based conversational patterns into a distributed coordination challenge 
where autonomous models directly edit host filesystems, manage parallel software
compilation tasks, and coordinate with remote virtual machines . The terminal 
has transitioned from a passive input-output window into a highly active, 
multi-agent control plane responsible for routing context, enforcing access 
controls, and rendering complex, overlapping workflow pipelines in real time . 

This brief outlines the engineering design, system placement, and precise 
recovery protocols for the next development arc of the Ridge terminal control 
plane, bridging local desktop interfaces, remote Local Area Network (LAN) or 
cloud nodes, and the unified `rdg` command-line utility . The architecture is 
designed around small, reversible changes, avoiding any modifications to the 
secure transport protocol layers or the single-source-of-truth configuration 
files.

---

## Workspace Layout and Interaction UX Polish

Managing multiple concurrent agents within a single workspace requires 
terminal-native structures that prevent mental fatigue and visual clutter . When
parallel agents run compile, test, and debugging cycles, the operator must 
immediately understand which agent requires attention, where resource 
bottlenecks are happening, and how state changes are moving through the terminal
splits .

| Workspace Viewport Component | Rendering State Variable | Active Performance 
Target | Input Protocol Compatibility |
| :--- | :--- | :--- | :--- |
| **Workspace Selector** | `active_workspace_id`  | Layout swap in $\le 
120\text{ms}$  | `Ctrl+1` through `Ctrl+9` hotkeys  |
| **Agents Rail Overlay** | `agent_process_matrix`  | Render update in $\le 
5\text{ms}$  | Read-only state monitoring  |
| **Command Modal** | `is_command_palette_open`  | Overlay painting in $\le 
15\text{ms}$  | `Ctrl+K` key combo  |
| **Reasoning Effort** | `model_effort_mode`  | Context swap on next transaction
| Interactive `/effort` toggle  |

### Work Item 1.1: Visual Agents Rail with Reasoning Effort Toggle and Context 
Parameter Auto-Injection

#### Problem Statement
In dense multi-pane environments, operators cannot easily see the operational 
states of background sub-agents, leading to lost time when processes idle or 
wait for inputs . Standard models also lack an integrated mechanical toggle to 
balance latency against deep reasoning, causing either high API costs or shallow
code edits . This is compounded by models generating incorrect assumptions when 
they lack explicit context files like `CLAUDE.md` and `AGENTS.md` .

#### Minimal Architecture Placement
* **UI Layer**: Multi-pane status bar renderer, fuzzy matching input modal, and 
cell-grid layout engine . No changes are made to the parser engine or transport 
sockets .

#### Comprehensive Implementation Details
The visual workspace features an interactive "Agents Rail" running vertically on
the left side of the terminal screen . This rail renders a real-time status 
matrix for all active agent subprocesses, showing their operational mode, token 
burn-down velocity, and current state (such as idle, running, or waiting for 
human input) . 

Operators can toggle the reasoning level of any agent at any time by typing the 
`/effort` command directly into the active pane, which switches between 
high-speed execution and deep-reasoning paths . 

To prevent models from making poor assumptions about local files, the workspace 
automatically scans the root folder for `CLAUDE.md` and `AGENTS.md` 
configuration templates upon startup . It dynamically injects these conventions 
directly into the agent's system prompt before triggering a run, reducing 
guessing and improving output reliability .

```
+-------------------------------------------------------------+
| [T] Local Workspace      [T*] Remote Staging Server         |
+-----------------------------+-------------------------------+
|                             |                               |
|   Active Developer Shell    |   Agent Pane (Claude Code)    |
|   (Local PTY Host)          |   [State: Awaiting Approval]  |
|                             |   ~~~~~~~~~~~~~~~~~~~~~~~~~~  |
|   $ cargo build             |   /policy review requested    |
|   Compiling... [OK]         |   | Halo Ring Pulse (Red) |   |
|                             |   ~~~~~~~~~~~~~~~~~~~~~~~~~~  |
+-----------------------------+-------------------------------+
|  TUI Command Overlay / Roster Picker (Ctrl+K)               |
|  > Select Roster: [ ] backend_dev  [ ] security_audit       |
+-------------------------------------------------------------+
```

#### Concrete Acceptance Tests

```
Step 1: Spawning Multi-Agent Workspace
  Execute client: $ rdg start-workspace --dir=/workspace/demo
  Assert: Workspace detects and reads /workspace/demo/CLAUDE.md .
  Assert: TUI renders vertical "Agents Rail" displaying inactive sub-agents .

Step 2: Triggering Execution with High-Reasoning Toggle
  In agent pane, execute command: /effort high
  Execute task: "Generate comprehensive Rust unit tests"
  Assert: System prompt prepends the conventions extracted from CLAUDE.md .
  Assert: Model configuration swaps to deep-reasoning mode with increased token 
budget .
  Assert: Visual rail updates agent state indicator to "Running" .

Step 3: Simulating User Approval Intercept
  Agent generates code and proposes writing to local disk .
  Assert: Active pane border displays a pulsing warning halo drawn from the 
theme's warning palette .
  Assert: Visual rail updates state indicator to "Awaiting Input" .
```

#### Risks and Non-Goals
* **Non-Goals**: Designing custom 3D animations, embedding graphic meshes, or 
managing user credentials inside the visual rail .
* **Risks**: High-frequency visual updates can degrade typing latency during 
rapid output streams . To prevent this, the terminal must use a dirty-bit row 
caching mechanism to skip drawing unchanged cell blocks .

---

### Work Item 1.2: Standard File Piping Parameters with Unified Execution 
Sequences

#### Problem Statement
Directly feeding data streams into autonomous agents or running multi-stage 
execution flows in standard terminals requires clumsy input redirection . 
Operators must manually copy and paste text or chain multiple shell wrappers 
together, which breaks workflow flow, risks command injection, and causes 
context fragmentation .

#### Minimal Architecture Placement
* **Adapter Layer**: Stream management middleware, standard I/O redirection 
adapter, and sequence execution runner .

#### Comprehensive Implementation Details
This work item implements standard file parameters (`-r` for reading and `-w` 
for writing) alongside a headless execution mode (`-y`) to turn the agent 
control plane into a composable tool within the Unix pipeline . The adapter 
translates streaming inputs directly into an active agent thread and routes 
model outputs back to standard stdout, enabling commands to be cleanly chained .

Complex, multi-stage sequences are handled using a unified execution syntax, 
`@sequence`, which allows operators to define explicit, multi-agent workflows . 
These sequences can specify where human decisions are required and determine 
when tasks can run in parallel, avoiding manual hands-offs .

```
+-----------------------------------------------------------------+
| Standard Pipe Data Stream                                       |
|   $ cat main.rs | rdg -r - -w refactored.rs -y                  |
+--------------------------------+--------------------------------+
                                 |
                                 v
+-----------------------------------------------------------------+
| Adapter Stream Layer                                            |
|   - Reads input stream into contextual memory          |
|   - Runs code translation headless via -y parameter   |
|   - Writes output to target file parameter            |
+-----------------------------------------------------------------+
```

#### Concrete Acceptance Tests

```
Step 1: Executing Standard Read Parameter
  Run pipeline: $ echo "fn test() {}" | rdg -r - "Document this function" -y
  Assert: Command execution completes non-interactively .
  Assert: Output streams directly to stdout with correct documentation blocks .

Step 2: Executing Workspace Write Parameter
  Run command: $ rdg -r src/main.rs "Optimise imports" -w src/main_opt.rs -y
  Assert: Output file "src/main_opt.rs" is created .
  Assert: Output file contains the corrected import code blocks .

Step 3: Executing Unified Execution Sequence
  Run sequence command: $ rdg @sequence configure-build.json
  Assert: Step 1 (Gemini architecture outline) launches and writes output .
  Assert: Step 2 (Claude code generation) reads output and writes target code .
  Assert: Step 3 (Deterministic validation tests) runs automatically .
```

#### Risks and Non-Goals
* **Non-Goals**: Managing file permissions, enforcing access controls on local 
storage paths, or handling external cloud backups.
* **Risks**: Chaining large file streams can exhaust the memory limit of the 
active agent session . To prevent crashes, the input adapter must enforce a 
strict, configurable size limit (e.g., 4MB) on streamed inputs .

---

## Connection Robustness, PTY Persistence, and Multi-Controller Arbitration

To maintain a reliable development workspace, active shell sessions and agent 
processes must run independently of client connection states . Separating the 
terminal presentation layer from the underlying process host ensures that 
terminal states persist through connection drops and allows multiple controllers
to attach safely .

| Connection Variable | Local Default (Desktop Socket) | Remote Default (SSH 
Target) | Backup Reconnect Strategy |
| :--- | :--- | :--- | :--- |
| **Transport Layer** | Local Unix Domain Socket  | TCP Tunnel / SSH Session  | 
Linear Reconnect Loop  |
| **Timeout Interval** | `None` (Persistent) | 30 Seconds  | Exponential Backoff
|
| **Authentication** | Local User Credentials | SSH Key / TLS Handshake  | 
Token-Bound Match  |
| **Arbiter State** | Active Console Keypresses | Programmatic CLI Requests  | 
Input Lock / Idle Out  |

### Work Item 2.1: Out-of-Process PTY Host Daemon and Terminal State 
Reattachment

#### Problem Statement
If an interactive client app crashes, closes, or loses network connection, the 
operating system sends a `SIGHUP` signal to active shell tasks and agent 
processes, terminating them instantly . This destroys long-running builds, 
active test environments, and agent session history .

#### Minimal Architecture Placement
* **Adapter Layer**: Persistent system service daemon running in the background,
managing master PTY handles and local Unix sockets . No changes are made to the 
local desktop app's rendering pipeline .

#### Comprehensive Implementation Details
To isolate running tasks from connection issues, a persistent background daemon,
`rdgd`, is introduced to act as the single owner of all master pseudo-terminal 
(PTY) files . When an interactive window opens or an operator connects via the 
`rdg` command-line utility, the client connects to the daemon's local UNIX 
socket rather than spawning processes directly . 

If the client disconnects, `rdgd` traps the `SIGHUP` signal, shields the child 
processes, and keeps all shells and agent tasks running in the background . The 
next time a client attaches, the daemon replays the logged scrollback and 
terminal state, allowing the operator to pick up right where they left off .

```
+---------------------------------------------------------------------+
| Ridge Desktop Client or CLI (Attached Viewport)                     |
+----------------------------------+----------------------------------+
                                   | (UNIX Socket / TCP Tunnel)
                                   v
+---------------------------------------------------------------------+
| Background Daemon (rdgd Process Host)                               |
|                                                                     |
|  +---------------------------------------------------------------+  |
|  | PTY Session Manager                                           |  |
|  |   - Controls child shells and catches SIGHUP        |  |
|  |   - Tracks folder paths and env details         |  |
|  |   - Replays screen state upon reconnection      |  |
|  +---------------------------------------------------------------+  |
+---------------------------------------------------------------------+
```

#### Concrete Acceptance Tests

```
Step 1: Starting Background Daemon
  Execute daemon: $ rdgd --socket /tmp/rdg-dev.sock
  Assert: Daemon starts, creates the socket file, and idles.

Step 2: Spawning Interactive Shell and Running Compilation
  Execute client: $ rdg attach --socket /tmp/rdg-dev.sock
  Run long build: $ cargo build --release
  Assert: Terminal displays compilation progress logs.

Step 3: Simulating Abrupt Client Disconnect
  Terminate client process: $ killall rdg
  Assert: Daemon log confirms client disconnected, leaving build running.

Step 4: Reattaching and Validating State Reassembly
  Execute client: $ rdg attach --socket /tmp/rdg-dev.sock
  Assert: Reattached session restores the active workspace layout .
  Assert: Scrollback displays the completed build output history .
```

#### Risks and Non-Goals
* **Non-Goals**: Rebuilding custom network transport protocols, designing custom
encryption algorithms, or syncing execution states across separate physical 
hosts .
* **Risks**: If the background daemon itself crashes, all active child sessions 
will be terminated. To minimize this risk, `rdgd` is designed as a minimal, 
lightweight service with no external UI dependencies .

---

### Work Item 2.2: Multi-Controller Input Arbitration and Concurrency State 
Locking

#### Problem Statement
When multiple controllers—such as a desktop app and a remote CLI session—attach 
to the same workspace simultaneously, concurrent typing or overlapping tool 
calls can corrupt the screen state and cause execution drift .

#### Minimal Architecture Placement
* **Core Layer**: Session lock manager, status coordinator, and user priority 
arbiter .

#### Comprehensive Implementation Details
The control plane implements a transactional, timestamp-based locking protocol 
inside the background daemon to handle concurrent connections . The system is 
modeled as a state machine where only one attached client can hold the active 
write lock at a time. 

If Client-A is typing or running an agent task, the daemon locks input for other
attached users . When a locked-out client tries to send inputs, those keystrokes
are blocked at the daemon layer, and the client receives an overlay warning 
indicating who is currently typing . The write lock automatically expires after 
a configurable idle period (e.g., 1500ms), returning the session to a shared 
state .

#### Concurrency Lock Transitions
Let client inputs be represented by operations $Op(C_k)$ where $C_k$ is the 
client identifier. The lock state $L$ operates within the domain 
$\{\text{Unlocked}, \text{Active}(C_i)\}$ with an associated idle timer $T$. The
arbitration logic handles incoming operations using the following state 
transition model:

$$L_{t+1} = \begin{cases} 
\text{Active}(C_k), & \text{if } L_t = \text{Unlocked} \\ 
\text{Active}(C_k), & \text{if } L_t = \text{Active}(C_k) \text{ (Reset Timer } 
T) \\
\text{Unlocked}, & \text{if } T \ge 1500\text{ms} \\
\text{Active}(C_i), & \text{if } L_t = \text{Active}(C_i) \land k \neq i \land T
< 1500\text{ms} \text{ (Reject Input)}
\end{cases}$$

#### Concrete Acceptance Tests

```
Step 1: Initializing Concurrent Client Attachments
  Attach local Client-A: $ rdg attach --session=dev-env
  Attach remote Client-B: $ rdg attach --session=dev-env

Step 2: Acquiring Session Lock
  Client-A sends input character stream: "git status"
  Assert: Client-A input is written successfully to the PTY.
  Assert: Lock state transitions to Active(Client-A).

Step 3: Intercepting Conflicting Input Stream
  Client-B attempts to write input character: "c"
  Assert: Client-B input is dropped and not sent to the PTY.
  Assert: Client-B terminal displays "Session locked by Client-A (local)" 
overlay warning.

Step 4: Releasing Session Lock via Idle Timeout
  Idle Client-A for 1600ms.
  Assert: Lock state resets to Unlocked.
  Client-B sends input character stream: "ls -la"
  Assert: Client-B input is written successfully to the PTY.
```

#### Risks and Non-Goals
* **Non-Goals**: Implementing multi-writer collaborative editing of the same 
command line, or managing multi-leader database replication schemes .
* **Risks**: High network latency can delay lock release updates for remote 
users . To mitigate this, clients run a local clock to predict lock timeouts and
avoid input lag .

---

## Team Collaboration: Secure HITL & Roster Orchestration

When giving autonomous agents access to production-adjacent development 
environments, the control plane must act as a strict execution barrier . It must
intercept shell calls, sanitize inputs, and request explicit approvals before 
running potentially destructive actions .

| Threat Category | Attack Vector | Security Mitigation | Control Plane 
Placement |
| :--- | :--- | :--- | :--- |
| **Tool Hijacking** | Indirect Prompt Injection  | Token-Bound Action 
Middleware  | Execution Interceptor  |
| **Command Obfuscation** | GuardFall Shell Escape  | Syntactic Shell 
Normalization  | Token Matcher  |
| **Rogue Write Access** | Unauthorized File Writes  | Path Boundary Validation 
| Policy Middleware  |
| **Workspace Contamination** | Shared Context Drift  | Git Worktree Sandboxes  
| File Virtualizer  |

### Work Item 3.1: Token-Bound Boundary Action Interceptor with Shell 
Deobfuscation

#### Problem Statement
AI agents can execute destructive commands due to indirect prompt injection or 
context confusion (such as recursive deletions outside the project workspace) 
before the human operator has a chance to review or block them . Simple text 
matches are easily bypassed by command obfuscation and shell variable expansion 
.

#### Minimal Architecture Placement
* **Core Layer**: Security middleware and policy evaluator located directly in 
the agent's tool execution path .
* **Adapter Layer**: Syntactic shell parsing deobfuscator and YAML rule matching
engine .

#### Comprehensive Implementation Details
This work item implements an in-process security framework that intercepts all 
agent actions before they reach the host environment . The system processes 
proposed commands through nine syntactic deobfuscation filters to resolve 
variable expansions, decode hex/octal escapes, handle C-style quoting, and 
resolve concatenated strings (e.g., converting `'r''m'` back to `rm`) . 

Once normalized, the command is checked against explicit YAML-defined policies 
to verify path limits and command rules . Safe read actions are allowed by 
default, while potentially destructive commands are held for operator review .

```
Raw Agent Command
  |
  v
+--------------------------------------------------------------+
| Deobfuscation Filter Subsystem                      |
|  - Decode Escapes (\xHH) & resolve assignments     |
|  - Merge concatenated quote slices ('r''m')        |
+------------------------------+-------------------------------+
                               |
                               v
                       Normalized Text
                               |
                               v
+--------------------------------------------------------------+
| Token-Bound Policy Evaluator                                 |
|  - Check path boundary and pattern rules       |
|  - Score risk: R = sum(w_i * pattern_match)                  |
+------------------------------+-------------------------------+
                               |
               +---------------+---------------+
               | R < Allow     | R >= Review   | R >= Block
               v               v               v
         [Execute PTY]   [Pause & Prompt] [Drop & Alert]
```

#### Concrete Acceptance Tests

```
Step 1: Deploying Obfuscated Path Escape Attempt
  Agent proposes command: LDIR="/etc" && e'c'ho "malicious" > $LDIR/passwd 
  Assert: Shell normalizer deobfuscates command to: echo "malicious" > 
/etc/passwd .
  Assert: Policy evaluator flags path violation outside workspace boundaries .
  Assert: Command execution is blocked; agent receives execution denial error .

Step 2: Triggering Non-Obfuscated Destructive Action
  Agent proposes command: rm -rf /workspace/demo/target
  Assert: Policy engine flags risk pattern "recursive-delete" .
  Assert: Execution pauses; system prompts human operator for approval .

Step 3: Verification of Human-In-The-Loop Approval
  Operator rejects the command.
  Assert: Command is dropped, and no changes are written to the PTY.
```

#### Risks and Non-Goals
* **Non-Goals**: Managing system-level user permissions or running full 
operating system process sandboxing .
* **Risks**: Parsing complex bash scripts can cause command lag. The 
normalization engine is kept to a focused subset of patterns commonly used to 
bypass security filters .

---

### Work Item 3.2: Multi-Teammate Pairing Portal and Hardware Secure approval

#### Problem Statement
When collaborating on a shared system, team members cannot easily view progress,
verify code changes, or approve sensitive tool operations in real-time . 
Traditional screen sharing lacks low-latency text selection, and sharing shell 
access directly with external team members risks credential exposure .

#### Minimal Architecture Placement
* **Adapter Layer**: WebRTC signaling integration and local Unix socket broker 
that replicates raw screen data to guest sessions .
* **UI Layer**: Local Ledger-style hardware key handler and authorization 
dispatcher .

#### Comprehensive Implementation Details
The control plane introduces a secure pairing portal that allows team members to
join interactive terminal sessions via WebRTC-encrypted connections . The 
session host can share a unique URL that projects the workspace layout and 
terminal splits directly into a browser-based view . 

Guests can be granted read-only view access or full write permissions, with 
pasted text delivered cleanly as a bracketed paste to prevent execution errors .

For sensitive environments, the system integrates physical hardware key 
validation (such as Ledger or YubiKey devices) . When an agent proposes a 
high-risk action, the host can require physical validation on their connected 
hardware key before the command is sent to the PTY .

```
+-----------------------------------------------+
| Secure Pairing Portal Gateway                 |
|   - Replicates terminal outputs via WebRTC    |
|   - Sends input events back to active session |
+-----------------------+-----------------------+
                        |
                        v
+-----------------------------------------------+
| Secure Multi-Teammate Workspace               |
|  - Validates guest credentials & permissions  |
|  - Delivers pasted text as bracketed pastes   |
+-----------------------+-----------------------+
                        | (High-Risk Action Flag)
                        v
+-----------------------------------------------+
| Hardware Validation Hook                      |
|  - Prompts for physical device confirmation  |
|  - Keys never leave the secure chip |
+-----------------------------------------------+
```

#### Concrete Acceptance Tests

```
Step 1: Generating Guest Sharing Link
  Host runs pairing command: $ rdg share --allow-write=false
  Assert: Signaling server establishes pairing tunnel and returns secure URL .

Step 2: Guest Connecting to Session
  Guest connects via the pairing URL in a web browser .
  Assert: Guest viewport mirrors terminal outputs and layout splits in real time
.
  Assert: Guest input actions are rejected due to read-only permissions .

Step 3: Verifying Hardware Key Approval Flow
  Agent proposes transaction command: $ rdg run-migration --prod
  Assert: Execution pauses; status bar displays "Confirm action on hardware key"
.
  Host presses confirm button on physical Ledger/YubiKey device .
  Assert: Cryptographic validation succeeds, and command runs on the PTY .
```

#### Risks and Non-Goals
* **Non-Goals**: Designing custom transport encryption protocols, building 
authentication databases, or handling user account registrations .
* **Risks**: WebRTC signaling can fail under restrictive corporate firewalls. To
ensure reliability, the adapter uses standard STUN/TURN relays to help clients 
connect .

---

## Multi-Host Foreign Terminals and Same-Workspace Layout Orchestration

Developers often coordinate tasks across multiple execution targets, such as 
local developer machines, staging containers, and remote virtual private servers
. Visual layout synchronization bridges these remote targets into a single, 
cohesive developer workspace .

| Layout Node Configuration | Sizing Mode Flag | Target Resolution | Rendering 
Component |
| :--- | :--- | :--- | :--- |
| **Local Desktop Panel** | `Native_Grid` | Client Viewport Pixels | Bevy/Wgpu 
GPU Renderer  |
| **Remote Tmux Split** | `manual`  | Fixed Cells (e.g., 120x40)  | Parsed 
Control Node  |
| **Foreign SSH Terminal** | `latest`  | Adapts to Active Viewport  | Re-wrapped
Text Grid  |
| **Sandbox Workspace** | `smallest`  | Constrained by Client Screen | PTY 
Virtual Layout  |

### Work Item 4.1: Tmux Control-Mode Bridge and Layout Tree Synchronizer

#### Problem Statement
Coordinating splits and window layouts across different remote hosts and local 
environments requires developers to jump between windows, causing context loss 
and slowing down workflows .

#### Minimal Architecture Placement
* **Adapter Layer**: Tmux control-mode parser interface integrated into the 
workspace's SSH connection module . This module communicates via `tmux -CC` to 
translate control-mode data streams into active workspace splits .

#### Comprehensive Implementation Details
To simplify remote workflows, the control plane includes a Tmux control-mode 
adapter . When connecting to a remote host via SSH, the adapter starts Tmux in 
control mode (`tmux -CC`), translating standard console outputs into a 
structured control stream . 

The adapter parses Tmux's layout commands and asynchronous notifications (such 
as `%begin`, `%end`, and `%layout-change`) to map the remote terminal layout 
directly into local workspace panels . This allows local splits and remote 
splits to be managed under a single unified view, routing input commands 
directly to the correct remote pane .

```
+-----------------------------------------------------------+
| Local Desktop Workspace                                   |
|                                                           |
|  +-----------------------------+-----------------------+  |
|  | Local Shell Pane            | Remote SSH Pane       |  |
|  |                             | (Tmux Bridge Control) |  |
|  | $ cargo test                | $ tail -f syslog      |  |
|  | Tests passed [24/24]  | 12:00:01 [INFO]   |  |
|  |                             |                       |  |
|  +-----------------------------+-----------------------+  |
+-----------------------------------------------------------+
                             |
                    (Tmux Control Mode)
                             v
+-----------------------------------------------------------+
| Remote Host Environment                                   |
|   - Tmux server parses input and updates PTY handles      |
|   - Translates layout updates into control-mode strings   |
+-----------------------------------------------------------+
```

#### Concrete Acceptance Tests

```
Step 1: Connecting to Remote Host in Control Mode
  Execute client: $ rdg connect SSHMUX:staging-srv
  Assert: System starts remote Tmux session using control mode: tmux -CC .
  Assert: Control plane parses remote layout trees into structured JSON .

Step 2: Syncing Split Layouts
  Assert: Local UI renders remote panes as native workspace panels .
  Assert: Programmatic actions (like split-pane) are sent as "split-window" to 
Tmux .

Step 3: Handling Disconnects
  Disconnect SSH connection.
  Assert: Remote PTY processes continue running on the server .
  Assert: Local UI updates status bar indicator to "Detached".
```

#### Risks and Non-Goals
* **Non-Goals**: Forwarding graphical interfaces (like X11 or Wayland), running 
custom window managers on remote hosts, or supporting Tmux versions older than 
3.2 .
* **Risks**: Network latency can delay screen updates under poor connection 
conditions . To minimize visual lag, the adapter caches screen updates locally 
and repaints the display once connection conditions stabilize .

---

### Work Item 4.2: Dynamic Git Worktree Sandbox and Context Domain Coordinator

#### Problem Statement
When running parallel agents or multi-agent swarms in the same project folder, 
concurrent file writes can cause file conflicts, dirty git index states, and 
lost changes .

#### Minimal Architecture Placement
* **Core Layer**: Workspace virtual file manager, workspace route coordinator, 
and git automation adapter .

#### Comprehensive Implementation Details
The control plane implements an automated isolation manager that prevents file 
conflicts between parallel agents . When a task is delegated to a roster of 
sub-agents, the system maps out a parallel execution plan . 

Instead of executing actions directly in the shared project directory, the 
isolation manager creates a dedicated Git worktree for each active agent thread 
. This creates fully isolated work branches, allowing each model to edit files 
and run test suites within its own sandbox without colliding with other agents .

By distributing tasks across these isolated "Context Domains," the control plane
can effectively partition large codebase tasks across a coordinated swarm of 
agents, scaling the effective context window . Once tasks are complete, the 
manager validates the changes and merges them back into the main branch, 
ensuring a clean git history .

```
+-----------------------------------------------------------------+
| Shared Project Main Directory                                   |
|   - Holds main branch state and master git config     |
+--------------------------------+--------------------------------+
                                 | (Delegate Parallel Tasks)
                                 v
+-----------------------------------------------------------------+
| Core Workspace Isolation Manager                                |
|  - Spawns parallel sub-agents into isolated worktrees |
|  - Routes distinct files to context domain modules    |
+--------------------------------+--------------------------------+
                                 |
         +-----------------------+-----------------------+
         v                                               v
+------------------------+                      +------------------------+
| Worktree Sandbox A     |                      | Worktree Sandbox B     |
| (Agent-1, db-refactor) |                      | (Agent-2, api-docs)    |
+------------------------+                      +------------------------+
```

#### Concrete Acceptance Tests

```
Step 1: Setting up Multi-Agent Roster Run
  Execute swarm command: $ rdg swarm-run --roster=feature-dev "Refactor API and 
update docs"
  Assert: Swarm coordinator partitions feature task into separate Context 
Domains .
  Assert: Git subsystem creates dedicated worktrees for Agent-1 and Agent-2 .

Step 2: Validating Code Sandbox Isolation
  Agent-1 refactors DB code inside its worktree sandbox .
  Agent-2 updates API documentation markdown in parallel .
  Assert: File edits and compiler runs operate independently without collision .

Step 3: Verification and Merging
  Run verification checks: $ rdg verify-changes
  Assert: Automated test suites pass for both sandbox worktrees .
  Assert: Isolation manager merges branches and closes sandbox worktrees.
```

#### Risks and Non-Goals
* **Non-Goals**: Replacing system-level containers (like Docker), managing raw 
disk partitions, or handling distributed git synchronization hosts .
* **Risks**: Creating multiple worktrees can consume significant disk space on 
large codebases. The manager must track worktree lifecycles and automatically 
clean up sandboxes once changes are merged .

---

## Failure Modes, Recovery, and Resilience Patterns

Building resilience into the control plane requires handling resource leaks, 
loop runaways, and unexpected transport failures gracefully . Robust safety 
guards must ensure execution stability and clean recovery paths .

```
                 +-----------------------------------+
                 |        Ridge Control Plane        |
                 +-----------------+-----------------+
                                   |
                                   | (Monitors CPU/Memory/Run Loops)
                                   v
                 +-----------------------------------+
                 |    Watchdog Monitoring Thread     |
                 +-----------------+-----------------+
                                   |
                  +----------------+----------------+
                  |                                 |
                  v [RSS > 512MB]                   v [Loop Count > 15]
        +---------+---------+             +---------+---------+
        |   Memory Watchdog |             |  Runaway Watchdog |
        +---------+---------+             +---------+---------+
                  |                                 |
                  | (SIGTERM -> SIGKILL)            | (Pause & Alert Operator)
                  v                                 v
        +---------+---------+             +---------+---------+
        |   Terminate Agent |             |   HITL Lockout    |
        +-------------------+             +-------------------+
```

### Work Item 5.1: Concurrency Viewport Sync Engine via Server-Authoritative 
Operational Transform

#### Problem Statement
Highly variable network latency between desktop client windows, LAN remote 
terminals, and CLI interfaces can cause rendering sync errors, screen tearing, 
and cursor hopping during active agent runs .

#### Minimal Architecture Placement
* **Core Layer**: Terminal display synchronizer, viewport coordinate map, and 
Operational Transform synchronization engine .

#### Comprehensive Implementation Details
The control plane implements a server-authoritative Operational Transform (OT) 
engine to handle terminal synchronization over high-latency networks . 

While decentralized CRDT models (like LWW-Register or OR-Set) are used to 
synchronize high-level workspace configuration states without a central 
coordinator , they carry high metadata overhead that can degrade performance 
under rapid text outputs . 

For rendering raw terminal buffers, the OT engine uses a server-authoritative 
model . The host daemon assigns sequential transaction numbers to all terminal 
display changes . When concurrent updates are received, the client transforms 
local changes against server-confirmed states . This maintains eventual 
consistency without screen tearing or cursor hopping .

#### Viewport Concurrency Model
The client-server viewport rendering engine implements a three-state operational
model :

$$\mathcal{S} = \langle A, X, Y \rangle$$

where $A$ represents the last server-confirmed layout state, $X$ tracks 
operations submitted but awaiting confirmation, and $Y$ represents unsubmitted 
local layout adjustments . When a remote screen update $Op_{\text{srv}}$ 
arrives, the local engine applies the transformation:

$$Op'_{\text{srv}} = \text{Transform}(Op_{\text{srv}}, X)$$

The layout engine applies $Op'_{\text{srv}}$ directly to state $A$, and 
recalculates the active screen render using:

$$\text{Viewport}_{\text{rendered}} = A \cdot \text{Transform}(X, 
Op_{\text{srv}}) \cdot \text{Transform}(Y, \text{Transform}(Op_{\text{srv}}, 
X))$$

#### Concrete Acceptance Tests

```
Step 1: Simulating High Network Latency and Packet Jitter
  Inject transport latency: 250ms latency with 15% packet loss.
  Attach local Client-A and remote Client-B to session .

Step 2: Executing Rapid Output Commands
  Run command: $ find /workspace -name "*.rs"
  Assert: Output stream is serialized with sequential transaction IDs .
  Assert: Viewport displays output progress smoothly on both clients .

Step 3: Simulating Concurrent Intersecting Actions
  Client-A triggers window-resize; Client-B attempts terminal scrollback query .
  Assert: Server-authoritative OT transforms resize operations cleanly .
  Assert: Both screens converge on the same viewport dimensions and layout state
.
```

#### Risks and Non-Goals
* **Non-Goals**: Designing a custom end-to-end encryption transport layer, or 
syncing non-terminal assets (like local image files) via the OT protocol .
* **Risks**: Extremely high network dropouts can cause sync delays. If the 
client gets out of sync for more than 5 seconds, the engine triggers a full 
layout repaint to restore consistency .

---

### Work Item 5.2: Watchdog Resource Monitors and Runaway Loop Guards

#### Problem Statement
Rogue agent threads can enter infinite loops, generate runaway tool calls, or 
exhaust system resources, leading to high API costs, workspace crashes, and 
system instability .

#### Minimal Architecture Placement
* **Core Layer**: Execution watchdog scheduler and host telemetry monitors 
running inside the background host process .

#### Comprehensive Implementation Details
The control plane implements an execution watchdog that monitors CPU load, 
memory usage, and tool execution frequencies in real-time . 

If an agent's memory footprint exceeds a configurable limit (e.g., 512MB), or if
the system detects the same tool being called repeatedly in an infinite loop, 
the watchdog intervenes . 

For runaway loops, the watchdog pauses execution and alerts the user . For 
resource exhaustion, the watchdog attempts a graceful termination via `SIGTERM` 
before escalating to a hard `SIGKILL` to protect host stability .

```
+-------------------------------------------------------------+
| Telemetry Monitor Thread                                    |
|   - Monitors active CPU, Memory, and Loop patterns          |
+------------------------------+------------------------------+
                               |
                               v
+-------------------------------------------------------------+
| Watchdog Rules Evaluator                                    |
|   - Check 1: Tool loop limit (Threshold > 15 iterations)    |
|   - Check 2: Session RSS limit (Threshold > 512MB)          |
+------------------------------+------------------------------+
                               |
               +---------------+---------------+
               | Memory Exceeded               | Run Loop Detected
               v                               v
        [Graceful SIGTERM]              [Pause Agent Session]
               |                               |
        (After 2s grace)                       v
               v                       [Prompt Operator HITL]
        [Force Kill Process]
```

#### Concrete Acceptance Tests

```
Step 1: Simulating Runaway Agent Loop
  Configure mock agent to run the "read-file" tool repeatedly.
  Assert: Watchdog loop counter tracks identical tool call iterations .
  Assert: On the 16th iteration, watchdog pauses the agent and sends an alert .

Step 2: Simulating Process Resource Exhaustion
  Simulate agent process memory usage exceeding 512MB .
  Assert: Watchdog issues a SIGTERM warning to the agent thread .
  Assert: Watchdog terminates the rogue process group after a 2-second grace 
period .

Step 3: Verification of Post-Failure State Survival
  Assert: Host PTY and active shell sessions remain responsive and running .
  Assert: System alerts the operator of the intervention via status bar 
notification .
```

#### Risks and Non-Goals
* **Non-Goals**: Rewriting system-level cgroups, or managing hardware 
virtualization limits.
* **Risks**: Aggressive watchdog thresholds can interrupt legitimate 
long-running tasks, like indexing a large codebase. Limits are configured via 
user-adjustable parameters inside the workspace setup to prevent false 
positives.

---

## Summary and Development Roadmap

To minimize engineering friction and support reversible integration steps, work 
items are prioritized below. The roadmap begins with core security controls and 
session hosting before building advanced multi-host layout and sharing 
capabilities.

```
                   +---------------------------------------+
                   |  PHASE 1: Core Foundation & Security  |
                   |  - Persistent Session Daemon (2.1)    |
                   |  - Boundary Action Interceptor (3.1)  |
                   +-------------------+-------------------+
                                       |
                                       v
                   +---------------------------------------+
                   |  PHASE 2: UX and Resilience           |
                   |  - Command Overlay Picker (1.2)       |
                   |  - Runaway Loop Watchdog (5.1)        |
                   |  - Auto-Reconnection Handler (5.2)    |
                   +-------------------+-------------------+
                                       |
                                       v
                   +---------------------------------------+
                   |  PHASE 3: Layout Sync & Collaboration |
                   |  - Pane Activity Halos (1.1)          |
                   |  - Tmux Sync Bridge (4.1)             |
                   |  - Viewport Resizing (4.2)            |
                   |  - Shared Socket Gateway (3.2)        |
                   +---------------------------------------+
```

The prioritized work items are organized in the engineering delivery schedule 
below to allow for iterative validation at each step.

| Implementation Sequence | Work Item | Targeted Layer Placement | Acceptance 
Validation Method | Core Dependency |
| :---: | :--- | :--- | :--- | :--- |
| **1** | **WI-2.1: Headless SIGHUP-Immune PTY Daemon** | Adapter Layer  | 
Automated PTY disconnect/reattach simulation  | Core Process Host |
| **2** | **WI-3.1: Token-Bound Action Interceptor** | Core Layer  | 
Deobfuscation execution verification scripts  | Normalizer Subsystem |
| **3** | **WI-5.1: Viewport Sync Engine** | Core Layer  | Latency injection 
network test suites  | Serializer Interface |
| **4** | **WI-1.1: Visual Agents Rail** | UI Layer  | Render engine state 
checks  | Status Matrix Router |
| **5** | **WI-5.2: Watchdog Resource Monitors** | Core Layer  | Simulating loop
execution and resource leaks  | Telemetry Interface |
| **6** | **WI-1.2: Standard File Piping Parameters** | Adapter Layer  | 
Standard input-output pipe routing tests  | CLI Stream Router |
| **7** | **WI-4.1: Tmux Control-Mode Bridge** | Adapter Layer  | Remote Tmux 
layout sync testing  | SSH Session Adapter |
| **8** | **WI-4.2: Git Worktree Sandboxes** | Core Layer  | Parallel feature 
branch compilation runs  | Git API Subsystem |
| **9** | **WI-3.2: Multi-Teammate Pairing Portal** | Adapter Layer  | Web 
socket screen mirroring verification  | WebRTC Signaling |

---
