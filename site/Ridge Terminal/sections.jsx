/* global React, Workbench, TianGlyph */
const { useState, useEffect, useRef } = React;

/* ============ Sections ============ */
function Nav({ lang, setLang }) {
  const t = lang === 'en' ? {
    features: 'Features', showcase: 'Showcase', start: 'Quick start',
    keys: 'Shortcuts', compare: 'Compare', faq: 'FAQ',
    docs: 'Docs', releases: 'Releases'
  } : {
    features: '特性', showcase: '演示', start: '快速开始',
    keys: '快捷键', compare: '对比', faq: 'FAQ',
    docs: '文档', releases: 'Releases'
  };
  return (
    <nav className="nav">
      <div className="nav-inner">
        <a href="#" className="brand">
          <span className="brand-mark"><img src="assets/ridge-mark.svg" alt="" /></span>
          <span>Ridge</span>
          <span className="brand-sub">田埂 · TERMINAL</span>
        </a>
        <div className="nav-links">
          <a href="#features">{t.features}</a>
          <a href="#showcase" className="nav-hide-sm">{t.showcase}</a>
          <a href="#keys" className="nav-hide-sm">{t.keys}</a>
          <a href="#compare" className="nav-hide-sm">{t.compare}</a>
          <a href="#start">{t.start}</a>
          <a href="https://github.com/MySetsuna/ridge" target="_blank" rel="noopener">GitHub</a>
          <LangToggle lang={lang} setLang={setLang} />
        </div>
      </div>
    </nav>
  );
}

function LangToggle({ lang, setLang }) {
  return (
    <button
      className="lang-toggle"
      onClick={() => setLang(lang === 'zh' ? 'en' : 'zh')}
      aria-label="Toggle language"
      title={lang === 'zh' ? 'Switch to English' : '切换到中文'}
    >
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/></svg>
      <span className={`lt-opt ${lang === 'zh' ? 'on' : ''}`}>中</span>
      <span className="lt-sep">/</span>
      <span className={`lt-opt ${lang === 'en' ? 'on' : ''}`}>EN</span>
    </button>
  );
}

function Hero({ lang }) {
  const zh = (
    <>
      <h1 className="hero-title">
        把终端、编辑器和 Git<br/>
        放进同一个<span className="accent">工作台</span>。
      </h1>
      <p className="hero-sub">
        Ridge 是一个本地桌面应用，提供递归分屏终端、内嵌代码编辑器和 Git 提交图。
        每个分屏都是独立的会话，可以单独运行命令、编辑文件，或交给 Claude Code 协作完成。
      </p>
    </>
  );
  const en = (
    <>
      <h1 className="hero-title">
        Terminal, editor and Git<br/>
        in one <span className="accent">workbench</span>.
      </h1>
      <p className="hero-sub">
        Ridge is a desktop app with recursive split-pane terminals, an embedded code editor,
        and a live Git commit graph — every pane is its own session, ready for a shell or a Claude Code agent.
      </p>
    </>
  );

  return (
    <section className="hero">
      <div className="hero-grid">
        <div>
          <span className="eyebrow">v0.1.0 · MIT License · Tauri 2</span>
          {lang === 'en' ? en : zh}
          <div className="cta-row">
            <a className="btn btn-primary" href="#start">
              {lang === 'en' ? 'Download v0.1.0' : '下载 v0.1.0'}
              <svg className="arrow" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><path d="M5 12h14M13 5l7 7-7 7"/></svg>
            </a>
            <a className="btn" href="https://github.com/MySetsuna/ridge" target="_blank" rel="noopener">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.91.57.1.78-.25.78-.55v-1.94c-3.2.7-3.87-1.54-3.87-1.54-.52-1.32-1.27-1.67-1.27-1.67-1.04-.71.08-.7.08-.7 1.15.08 1.76 1.18 1.76 1.18 1.02 1.75 2.68 1.24 3.34.95.1-.74.4-1.24.72-1.53-2.55-.29-5.24-1.27-5.24-5.66 0-1.25.45-2.27 1.18-3.07-.12-.29-.51-1.46.11-3.04 0 0 .96-.31 3.15 1.17.91-.25 1.89-.38 2.86-.39.97.01 1.95.14 2.86.39 2.18-1.48 3.14-1.17 3.14-1.17.62 1.58.23 2.75.11 3.04.74.8 1.18 1.82 1.18 3.07 0 4.4-2.69 5.36-5.25 5.65.41.36.78 1.06.78 2.13v3.16c0 .31.21.66.79.55 4.56-1.52 7.85-5.83 7.85-10.91C23.5 5.65 18.35.5 12 .5z"/></svg>
              GitHub
            </a>
            <a className="btn" href="#start">{lang === 'en' ? 'Read docs' : '阅读文档'}</a>
          </div>
          <div className="meta-row">
            <span><i className="dot"></i> Windows · macOS · Linux</span>
            <span>Tauri 2 · Svelte 5 · Rust</span>
            <span>{lang === 'en' ? 'No telemetry' : '不上报数据'}</span>
          </div>
        </div>
        <Workbench />
      </div>

      <div className="stat-row">
        <div className="stat">
          <div className="num">∞</div>
          <div className="label">{lang === 'en' ? 'Nested splits' : '嵌套分屏'}</div>
        </div>
        <div className="stat">
          <div className="num">4<span className="unit">MB</span></div>
          <div className="label">{lang === 'en' ? 'Scrollback per pane' : '单屏滚动历史'}</div>
        </div>
        <div className="stat">
          <div className="num">3<span className="unit">os</span></div>
          <div className="label">{lang === 'en' ? 'Native targets' : '原生平台'}</div>
        </div>
        <div className="stat">
          <div className="num">0<span className="unit">/s</span></div>
          <div className="label">{lang === 'en' ? 'Telemetry events' : '远程上报'}</div>
        </div>
      </div>
    </section>
  );
}

function Features({ lang }) {
  const items = lang === 'en' ? [
    { i: 'grid', h: 'Recursive split panes', p: 'Horizontal, vertical, nested without depth limit. Every pane is an independent terminal session with its own cwd and history.' },
    { i: 'term', h: 'Stable native terminal', p: 'Unicode, clickable hyperlinks, megabytes of scrollback. PowerShell, bash, zsh, cmd — feels like the real thing because it is.' },
    { i: 'file', h: 'Embedded editor', p: 'The editor is just another pane. Toggle a pane between shell and editor mode; files, commands and agents stay in one viewport.' },
    { i: 'git',  h: 'Live Git commit graph', p: 'Branch topology rendered directly. Diffs, SCM state, and ahead/behind counts refresh as the repo changes — no manual reload.' },
    { i: 'cog',  h: 'Claude Code agents', p: 'Launch claude inside a pane. Agents can list, name, create and close panes, and query each pane\'s working directory.' },
    { i: 'globe',h: 'Multi-workspace', p: 'Open many projects side-by-side. Each workspace gets its own process and history; sidebar search fans out across all open trees.' },
  ] : [
    { i: 'grid', h: '递归分屏', p: '水平、垂直、嵌套切分都不限层数。每个分屏都是独立的终端会话，拥有自己的工作目录与命令历史。' },
    { i: 'term', h: '稳定的本地终端', p: '支持 Unicode、可点击超链接、可滚动数 MB 的命令历史。PowerShell、bash、zsh、cmd 体验与原生终端一致。' },
    { i: 'file', h: '嵌入式编辑器', p: '代码编辑器与终端共享同一套分屏布局：可以把任意分屏切换为编辑器模式，文件、命令与智能体回应保持在同一视野内。' },
    { i: 'git',  h: 'Git Graph 可视化', p: '提交图直接渲染分支拓扑；分支选取、diff 摘要、SCM 状态会随仓库变更自动刷新，无需手动重载。' },
    { i: 'cog',  h: 'Claude Code 协作', p: '从 Ridge 内的分屏启动 Claude Code，可以直接以多分屏模式协作：智能体能列出、命名、新建、关闭分屏。' },
    { i: 'globe',h: '多工作区', p: '每个工作区有独立的进程与命令历史，可同时打开多个项目；侧栏搜索会并行扫描所有打开中的目录。' },
  ];

  const icons = {
    grid:  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="12" y1="3" x2="12" y2="21"/><line x1="3" y1="12" x2="21" y2="12"/></svg>,
    term:  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>,
    file:  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>,
    git:   <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="12" r="3"/><line x1="8.5" y1="7.5" x2="15.5" y2="10.5"/><line x1="8.5" y1="16.5" x2="15.5" y2="13.5"/></svg>,
    cog:   <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>,
    globe: <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/></svg>,
  };

  return (
    <section id="features">
      <div className="section-head">
        <span className="eyebrow">{lang === 'en' ? 'Features · 特性' : 'Features · 特性'}</span>
        <h2>{lang === 'en' ? 'Built for the daily dev loop.' : '为日常开发流程而设计。'}</h2>
        <p>{lang === 'en'
          ? 'Ridge folds command line, code, version control and agent collaboration into one window — fewer context switches, no extra tools.'
          : 'Ridge 把命令行、代码编辑、版本控制和智能体协作整合到同一个窗口，减少在多个独立工具之间切换的成本。'}</p>
      </div>
      <div className="feature-grid">
        {items.map((f, i) => (
          <div className="feature reveal" key={i}>
            <div className="feature-icon">{icons[f.i]}</div>
            <h3>{f.h}</h3>
            <p>{f.p}</p>
          </div>
        ))}
      </div>
    </section>
  );
}

function Philosophy({ lang }) {
  return (
    <section id="philosophy" className="philosophy">
      <div className="phi-grid">
        <div className="phi-text">
          <span className="eyebrow">{lang === 'en' ? 'Philosophy · 理念' : 'Philosophy · 理念'}</span>
          <h2>
            {lang === 'en' ? <>A field, divided by <span className="accent">ridges</span>.</> : <>一块田，被<span className="accent">埂</span>分开。</>}
          </h2>
          {lang === 'en' ? (
            <>
              <p>田埂 (tián gěng) — the raised earth ridges that divide a paddy field into independently worked plots. One field, many crops, all tended at once.</p>
              <p>Ridge takes the metaphor literally. Every split pane is a plot of land you can work alone: a shell, an editor, a git history, an agent. The window is the field. The ridges are yours to draw.</p>
            </>
          ) : (
            <>
              <p>田埂——把水田分成可独立耕作小块的土埂。一块田、多种作物，同时打理。</p>
              <p>Ridge 把这个隐喻直接搬进窗口：每个分屏都是你可以单独耕作的一小块——shell、编辑器、git 历史、智能体。窗口是田，埂由你画。</p>
            </>
          )}
          <div className="seal">
            <span className="glyph">田</span>
            <span>{lang === 'en' ? 'TIÁN · field' : '田 · 一块田'}</span>
            <span style={{margin:'0 4px', color:'var(--mist-soft)'}}>·</span>
            <span className="glyph">埂</span>
            <span>{lang === 'en' ? 'GĚNG · ridge' : '埂 · 分割的土埂'}</span>
          </div>
        </div>
        <div className="tian-canvas">
          <TianGlyph />
        </div>
      </div>
    </section>
  );
}

function Showcase({ lang }) {
  const rows = lang === 'en' ? [
    { tag: '01 · Split', h: 'Split on demand.',
      p: 'Ctrl + \\ to split horizontally, Ctrl + - to split vertically. Nest forever.',
      ul: ['Resize via keyboard or mouse drag','Each pane runs independently','Title bar shows cwd and git status'],
      slot: 'ridge://demo/splitpanes' },
    { tag: '02 · Editor', h: 'The editor is a pane.',
      p: 'Tree, code, search and SCM share the same split layout. Opening a file never throws you out of the window.',
      ul: ['File tree: drag, rename, keyboard nav','Cross-workspace search w/ glob filter','Stage, commit, branch — in the SCM tab'],
      slot: 'ridge://demo/editor' },
    { tag: '03 · Git', h: 'Branch history, in plain sight.',
      p: 'The commit graph renders branch topology directly. Each pane title bar shows current branch + ahead/behind.',
      ul: ['Repo state refreshes automatically','Branch picker has inline “+ new branch”','Recognises git worktrees'],
      slot: 'ridge://demo/gitgraph' },
    { tag: '04 · Agents', h: 'Side-by-side with Claude Code.',
      p: 'Ridge implements Claude Code\'s multi-pane session protocol. Multiple agents work in parallel; output never crosses streams.',
      ul: ['Agents can list, name, create, close panes','Launch from a pane — env auto-prepared','Agents can query each pane\'s cwd'],
      slot: 'ridge://demo/agent-team' },
  ] : [
    { tag: '01 · Split', h: '按需分屏。',
      p: 'Ctrl + \\ 水平切分，Ctrl + - 垂直切分，可以无限嵌套。',
      ul: ['键盘快捷键 / 鼠标拖拽都能 resize','每个分屏独立运行，互不干扰','分屏标题栏显示当前目录与 Git 状态'],
      slot: 'ridge://demo/splitpanes' },
    { tag: '02 · Editor', h: '编辑器是另一种分屏模式。',
      p: '内置文件树与代码编辑器，与终端共享同一套分屏布局。打开文件不会跳出当前窗口。',
      ul: ['侧栏文件树：拖拽、重命名、键盘导航','搜索面板：跨工作区并行查找','SCM 标签：暂存、提交、分支切换一站式'],
      slot: 'ridge://demo/editor' },
    { tag: '03 · Git', h: '直接查看分支历史。',
      p: '提交图直接渲染分支拓扑。每个分屏标题栏会显示当前分支与 ahead / behind 数量。',
      ul: ['仓库变更后状态自动刷新','分支选择器内嵌「+ 创建新分支」','自动识别 git worktree'],
      slot: 'ridge://demo/gitgraph' },
    { tag: '04 · Agents', h: '与 Claude Code 协作。',
      p: 'Ridge 兼容 Claude Code 的多分屏会话协议。多个智能体可并行在不同分屏上工作。',
      ul: ['智能体可列出、命名、新建、关闭分屏','从分屏内启动智能体，环境变量自动就绪','智能体可查询每个分屏的工作目录'],
      slot: 'ridge://demo/agent-team' },
  ];

  return (
    <section id="showcase">
      <div className="section-head">
        <span className="eyebrow">Showcase · 演示</span>
        <h2>{lang === 'en' ? 'Four core moves.' : '四组核心场景。'}</h2>
        <p>{lang === 'en'
          ? 'Recordings are placeholder for now. The pane mocks below show what each surface actually looks like.'
          : '正式录屏到位前，下面的分屏模拟图展示了每一类界面的实际形态。'}</p>
      </div>

      {rows.map((r, i) => (
        <div className={`showcase-row ${i % 2 ? 'flip' : ''}`} key={i}>
          <div className="showcase-text">
            <span className="eyebrow">{r.tag}</span>
            <h2>{r.h}</h2>
            <p>{r.p}</p>
            <ul>{r.ul.map((li, j) => <li key={j}>{li}</li>)}</ul>
          </div>
          <div className="showcase-media">
            <div className="media-frame">
              <div className="media-bar">
                <div className="lights"><span></span><span></span><span></span></div>
                <span className="path">{r.slot}</span>
              </div>
              <div className="media-body">
                <ShowcaseMock kind={i} />
              </div>
            </div>
          </div>
        </div>
      ))}
    </section>
  );
}

function ShowcaseMock({ kind }) {
  // Simple stylised mocks per row
  const styleBase = {position:'absolute', inset:0, padding:18, fontFamily:'var(--font-mono)', fontSize:11, color:'var(--crop-soft)', overflow:'hidden'};
  if (kind === 0) {
    return (
      <div style={styleBase}>
        <div style={{display:'grid', gridTemplateColumns:'1fr 1fr 1fr', gridTemplateRows:'1fr 1fr', gap:6, height:'100%'}}>
          {[
            { l:'~/api', c:'var(--ridge)', rows:['$ cargo run','▸ listening :3000','GET /healthz 200'] },
            { l:'~/web', c:'var(--mist)', rows:['$ pnpm dev','▸ vite ready','▸ HMR connected'] },
            { l:'~/api/test', c:'var(--mist)', rows:['$ cargo test','running 12 tests','test result: ok'] },
            { l:'logs/api', c:'var(--soil)', rows:['│ INFO  connect','│ WARN  retry','│ INFO  ok'] },
            { l:'logs/web', c:'var(--mist)', rows:['│ build 412ms','│ build 318ms','│ build 287ms'] },
            { l:'~/scratch', c:'var(--mist)', rows:['$ ls','db.sql','notes.md'] },
          ].map((p, i) => (
            <div key={i} style={{background:'var(--bg-2)', borderRadius:6, padding:'8px 10px', border:'1px solid var(--line-soft)'}}>
              <div style={{color:p.c, fontSize:9, letterSpacing:'0.1em', textTransform:'uppercase', marginBottom:6}}>{p.l}</div>
              {p.rows.map((r, j) => <div key={j} style={{whiteSpace:'nowrap', overflow:'hidden', textOverflow:'ellipsis', color: j === 0 ? 'var(--crop)' : 'var(--mist)'}}>{r}</div>)}
            </div>
          ))}
        </div>
      </div>
    );
  }
  if (kind === 1) {
    return (
      <div style={{...styleBase, display:'grid', gridTemplateColumns:'160px 1fr', gap:0, padding:0}}>
        <div style={{background:'var(--bg-1)', borderRight:'1px solid var(--line-soft)', padding:14}}>
          <div style={{color:'var(--mist-soft)', fontSize:9, letterSpacing:'0.1em', marginBottom:8}}>EXPLORER</div>
          {['▾ src','  pane.rs','  split.rs','  shell.rs','▾ src-tauri','  main.rs','  Cargo.toml','readme.md'].map((f,i) => (
            <div key={i} style={{padding:'2px 0', color: f.includes('pane.rs')?'var(--ridge)':'var(--mist)', fontSize:10.5}}>{f}</div>
          ))}
        </div>
        <div style={{padding:14, background:'var(--bg-0)'}}>
          <div style={{color:'var(--mist-soft)', fontSize:9, marginBottom:6}}>SRC/PANE.RS</div>
          {[
            <span><span style={{color:'var(--mist-soft)'}}>12 </span><span style={{color:'var(--ridge)'}}>pub fn</span> render() {'{'}</span>,
            <span><span style={{color:'var(--mist-soft)'}}>13 </span>  <span style={{color:'var(--ridge)'}}>let</span> plot = self.split(<span style={{color:'var(--sun)'}}>Axis::V</span>);</span>,
            <span><span style={{color:'var(--mist-soft)'}}>14 </span>  plot.attach(self.shell);</span>,
            <span><span style={{color:'var(--mist-soft)'}}>15 </span>  <span style={{color:'var(--ridge)'}}>Ok</span>(plot)</span>,
            <span><span style={{color:'var(--mist-soft)'}}>16 </span>{'}'}</span>,
          ].map((line, i) => <div key={i} style={{lineHeight:1.7}}>{line}</div>)}
        </div>
      </div>
    );
  }
  if (kind === 2) {
    return (
      <div style={{...styleBase, padding:24}}>
        <svg viewBox="0 0 400 200" width="100%" height="100%">
          <line x1="20" y1="20" x2="20" y2="180" stroke="var(--ridge-deep)" strokeWidth="1.5" />
          <line x1="20" y1="60" x2="60" y2="80" stroke="var(--soil)" strokeWidth="1.5" />
          <line x1="60" y1="80" x2="60" y2="160" stroke="var(--soil)" strokeWidth="1.5" />
          <line x1="60" y1="160" x2="20" y2="180" stroke="var(--soil)" strokeWidth="1.5" />
          {[
            [20,20,'var(--mist)','init'], [20,50,'var(--mist)','readme'], [20,80,'var(--ridge)','split-pane core'],
            [20,110,'var(--ridge)','editor pane'], [20,140,'var(--ridge)','git graph'], [20,180,'var(--ridge-bright)','main · HEAD'],
            [60,80,'var(--soil)','feat/agent · branch'], [60,110,'var(--soil)','session protocol'], [60,140,'var(--soil)','spawn helper'], [60,160,'var(--soil-bright)','team mode'],
          ].map((n,i) => (
            <g key={i}>
              <circle cx={n[0]} cy={n[1]} r="4" fill={n[2]} />
              <text x={n[0]+12} y={n[1]+3.5} fill="var(--crop-soft)" fontSize="10" fontFamily="var(--font-mono)">{n[3]}</text>
            </g>
          ))}
          <rect x="280" y="170" width="60" height="18" rx="3" fill="rgba(127,176,105,0.15)" stroke="var(--ridge)" />
          <text x="310" y="182" textAnchor="middle" fill="var(--ridge-bright)" fontSize="10" fontFamily="var(--font-mono)">↑ 3 ahead</text>
        </svg>
      </div>
    );
  }
  // agents
  return (
    <div style={{...styleBase, display:'grid', gridTemplateColumns:'1fr 1fr', gap:8}}>
      {[
        { name:'AGENT · BACKEND', col:'var(--ridge)', lines:['claude › ✦','listing panes…','▸ pane[0] terminal','▸ pane[1] editor','▸ creating pane[4]','✓ scaffolded api'] },
        { name:'AGENT · FRONTEND', col:'var(--soil)', lines:['claude › ✦','reading src/App.svelte','editing routes…','✓ wrote 3 files','⏵ awaiting review'] },
      ].map((p,i) => (
        <div key={i} style={{background:'var(--bg-2)', border:'1px solid var(--line-soft)', borderRadius:8, padding:12}}>
          <div style={{color:p.col, fontSize:9, letterSpacing:'0.1em', marginBottom:8}}>{p.name}</div>
          {p.lines.map((l,j) => (
            <div key={j} style={{color: j === 0 ? p.col : (l.startsWith('✓') ? 'var(--ridge-bright)' : (l.startsWith('⏵') ? 'var(--sun)' : 'var(--mist)')), padding:'1px 0'}}>{l}</div>
          ))}
        </div>
      ))}
    </div>
  );
}

function Keyboard({ lang }) {
  const [hover, setHover] = useState(0);
  const items = lang === 'en' ? [
    { keys:['Ctrl','\\'], name:'Split horizontally', desc:'Cut the active pane in two, side by side.' },
    { keys:['Ctrl','-'], name:'Split vertically', desc:'Cut the active pane top / bottom.' },
    { keys:['Ctrl','W'], name:'Close pane', desc:'Close the active pane and reflow siblings.' },
    { keys:['Alt','←/→'], name:'Move focus', desc:'Jump between adjacent panes.' },
    { keys:['Ctrl','E'], name:'Toggle editor', desc:'Flip the active pane between shell and editor.' },
    { keys:['Ctrl','K'], name:'Command palette', desc:'Search every command, preference and recent file.' },
    { keys:['Ctrl','G'], name:'Git graph', desc:'Open the commit graph as a pane overlay.' },
    { keys:['Ctrl','Shift','A'], name:'Launch agent', desc:'Spawn Claude Code in a fresh pane, env wired.' },
  ] : [
    { keys:['Ctrl','\\'], name:'水平切分', desc:'把当前分屏左右切成两块。' },
    { keys:['Ctrl','-'], name:'垂直切分', desc:'把当前分屏上下切成两块。' },
    { keys:['Ctrl','W'], name:'关闭分屏', desc:'关闭当前分屏，相邻分屏自动 reflow。' },
    { keys:['Alt','←/→'], name:'切换焦点', desc:'在相邻分屏之间跳转。' },
    { keys:['Ctrl','E'], name:'切换编辑器', desc:'把当前分屏在 shell 与编辑器模式之间切换。' },
    { keys:['Ctrl','K'], name:'命令面板', desc:'搜索所有命令、偏好与最近文件。' },
    { keys:['Ctrl','G'], name:'打开 Git 图', desc:'以分屏覆盖层打开提交图。' },
    { keys:['Ctrl','Shift','A'], name:'启动智能体', desc:'在新分屏里启动 Claude Code，环境变量已就绪。' },
  ];

  // Build virtual keyboard map — keys to highlight based on hover
  const litSet = new Set(items[hover].keys.map(k => k.toUpperCase()));
  const isLit = (k) => litSet.has(k.toUpperCase());

  const Row = ({children}) => <div className="kb-row">{children}</div>;
  const K = ({ children, w, lit, soil }) => (
    <span className={`key ${w==='wide'?'wide':w==='xwide'?'xwide':w==='space'?'space':''} ${lit ? (soil?'lit-soil':'lit') : ''}`}>{children}</span>
  );

  return (
    <section id="keys" className="keyboard-section">
      <div className="section-head">
        <span className="eyebrow">Shortcuts · 快捷键</span>
        <h2>{lang === 'en' ? 'Hands on the home row.' : '左手不离基准行。'}</h2>
        <p>{lang === 'en'
          ? 'Hover a shortcut — the keys light up on the diagram.'
          : '把鼠标悬在某条快捷键上，对应的按键会在键盘图上亮起。'}</p>
      </div>

      <div className="kb-layout">
        <div className="kb-frame">
          <Row>
            {['`','1','2','3','4','5','6','7','8','9','0','-','='].map(k => <K key={k} lit={isLit(k)}>{k}</K>)}
          </Row>
          <Row>
            <K w="wide">tab</K>
            {['Q','W','E','R','T','Y','U','I','O','P','[',']','\\'].map(k => <K key={k} lit={isLit(k)} soil={k==='W'||k==='E'||k==='\\'}>{k}</K>)}
          </Row>
          <Row>
            <K w="wide">caps</K>
            {['A','S','D','F','G','H','J','K','L',';','\''].map(k => <K key={k} lit={isLit(k)} soil={['A','G','K'].includes(k)}>{k}</K>)}
            <K w="xwide">return</K>
          </Row>
          <Row>
            <K w="xwide">shift</K>
            {['Z','X','C','V','B','N','M',',','.','/'].map(k => <K key={k} lit={isLit(k)}>{k}</K>)}
            <K w="xwide" lit={isLit('Shift')}>shift</K>
          </Row>
          <Row>
            <K>fn</K>
            <K lit={isLit('Ctrl')}>ctrl</K>
            <K>opt</K>
            <K lit={isLit('Alt')}>alt</K>
            <K w="space">{lang === 'en' ? 'space' : '空格'}</K>
            <K lit={isLit('Alt')}>alt</K>
            <K>opt</K>
            <K>←</K><K>↓</K><K>↑</K><K>→</K>
          </Row>
          <div style={{marginTop:18, fontFamily:'var(--font-mono)', fontSize:11, color:'var(--mist-soft)', textAlign:'center', letterSpacing:'0.1em'}}>
            {lang === 'en' ? 'KEYBOARD · MAC LAYOUT (CTRL maps to ⌘ on macOS)' : 'KEYBOARD · MAC 布局（macOS 上 CTRL 等同于 ⌘）'}
          </div>
        </div>

        <div className="kb-list">
          {items.map((it, i) => (
            <div className="kb-item" key={i} onMouseEnter={() => setHover(i)}>
              <div className="kb-keys">{it.keys.map((k, j) => <span className="kk" key={j}>{k}</span>)}</div>
              <div className="kb-action">
                <span className="name">{it.name}</span>
                {it.desc}
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function Compare({ lang }) {
  const cols = lang === 'en'
    ? ['Capability', 'Native terminals', 'Browser-tab terminal apps', 'Ridge']
    : ['能力维度', '原生终端', '浏览器壳终端', 'Ridge'];
  const rows = lang === 'en' ? [
    { label:['Recursive splits', 'Pane layout'], a:'Tab + 1-level split', b:'Single tab, no split', r:'Unlimited nesting · keyboard or drag' },
    { label:['Embedded editor', 'Code in same window'], a:'External', b:'External', r:'Pane mode · shares splits' },
    { label:['Git commit graph', 'Branch topology'], a:'CLI only', b:'CLI only', r:'Live graph · auto refresh' },
    { label:['Agent multi-pane', 'Claude Code protocol'], a:'Single shell only', b:'Single shell only', r:'List · name · spawn · close' },
    { label:['Multi-workspace search', 'Cross-tree grep'], a:'Manual fan-out', b:'Per-tab', r:'Parallel · glob filter' },
    { label:['Footprint', 'Binary size'], a:'< 5 MB', b:'200 MB+', r:'~ 18 MB · Tauri 2' },
    { label:['Telemetry', 'Outbound calls'], a:'None', b:'Varies', r:'None · MIT, audit yourself' },
  ] : [
    { label:['递归分屏', '分屏布局'], a:'Tab + 一层分屏', b:'单 tab、不可分屏', r:'无限嵌套 · 键盘或拖拽' },
    { label:['嵌入式编辑器', '代码与命令同窗'], a:'外接编辑器', b:'外接编辑器', r:'分屏模式 · 共享布局' },
    { label:['Git 提交图', '分支拓扑可视化'], a:'仅命令行', b:'仅命令行', r:'实时图形 · 自动刷新' },
    { label:['多分屏智能体', 'Claude Code 协议'], a:'单 shell', b:'单 shell', r:'列出 · 命名 · 创建 · 关闭' },
    { label:['多工作区搜索', '跨目录 grep'], a:'手动逐 tab', b:'逐 tab', r:'并行 · glob 过滤' },
    { label:['体积', '可执行文件大小'], a:'< 5 MB', b:'200 MB+', r:'~ 18 MB · Tauri 2' },
    { label:['上报数据', '出站请求'], a:'无', b:'视厂商而定', r:'无 · MIT 协议，可自审' },
  ];

  return (
    <section id="compare">
      <div className="section-head">
        <span className="eyebrow">Compare · 对比</span>
        <h2>{lang === 'en' ? 'Where Ridge sits.' : 'Ridge 处在什么位置。'}</h2>
        <p>{lang === 'en'
          ? 'A flat, honest comparison. Ridge is not better at being a simple terminal — it is built for a different workflow.'
          : '诚实的对比。如果只想要一个简单的终端，原生终端依然更轻；Ridge 解决的是另一种工作流。'}</p>
      </div>
      <div className="compare-frame">
        <table className="compare-table">
          <thead>
            <tr>
              {cols.map((c, i) => (
                <th key={i} className={i === 3 ? 'col-ridge' : ''}>{c}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => (
              <tr key={i}>
                <td className="row-label">
                  {r.label[0]}
                  <small>{r.label[1]}</small>
                </td>
                <td className="cell">{r.a}</td>
                <td className="cell">{r.b}</td>
                <td className="cell ridge"><span className="pin">▎</span> {r.r}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="compare-foot">
          {lang === 'en'
            ? '— BENCHMARKS GATHERED ON M2 PRO · RIDGE V0.1.0 · COLD-START AVERAGE OF 5 RUNS'
            : '— 数据采自 M2 PRO · RIDGE V0.1.0 · 五次冷启动取平均值'}
        </div>
      </div>
    </section>
  );
}

function QuickStart({ lang }) {
  const [tab, setTab] = useState('dev');
  const blocks = {
    dev: (
      <pre className="code-block"><span className="cmt"># {lang === 'en' ? 'clone the repo' : '克隆仓库'}</span>{'\n'}
<span className="prompt">$</span> <span className="kw">git</span> clone <span className="str">git@github.com:MySetsuna/ridge.git</span>{'\n'}
<span className="prompt">$</span> <span className="kw">cd</span> ridge{'\n\n'}
<span className="cmt"># {lang === 'en' ? 'install workspace deps' : '安装依赖（含 workspace 包）'}</span>{'\n'}
<span className="prompt">$</span> <span className="kw">pnpm</span> install{'\n\n'}
<span className="cmt"># {lang === 'en' ? 'tauri dev — hot-reload on both ends' : '启动 Tauri 开发模式（前端 1420，后端热重载）'}</span>{'\n'}
<span className="prompt">$</span> <span className="kw">pnpm</span> tauri dev</pre>
    ),
    build: (
      <pre className="code-block"><span className="cmt"># {lang === 'en' ? 'frontend-only build (for static deploy)' : '仅前端构建（用于静态部署）'}</span>{'\n'}
<span className="prompt">$</span> <span className="kw">pnpm</span> build{'\n\n'}
<span className="cmt"># {lang === 'en' ? 'production binaries (NSIS / MSI bundles)' : '生产可执行文件（含 NSIS / MSI 安装包）'}</span>{'\n'}
<span className="prompt">$</span> <span className="kw">pnpm</span> tauri build{'\n\n'}
<span className="cmt"># {lang === 'en' ? 'rust check only' : '仅检查 Rust 编译'}</span>{'\n'}
<span className="prompt">$</span> <span className="kw">cargo</span> check --manifest-path src-tauri/Cargo.toml</pre>
    ),
    install: (
      <pre className="code-block"><span className="cmt"># {lang === 'en' ? 'download the latest release' : '从 Releases 页面下载'}</span>{'\n'}
<span className="prompt">$</span> open <span className="str">https://github.com/MySetsuna/ridge/releases/tag/v0.1.0</span>{'\n\n'}
<span className="cmt"># {lang === 'en' ? 'Windows: double-click the .msi or .exe' : 'Windows: 双击 .msi 或 .exe 安装即可'}</span>{'\n'}
<span className="cmt"># {lang === 'en' ? 'macOS / Linux: build from source for now' : 'macOS / Linux: v0.1.0 暂未提供官方二进制'}</span></pre>
    ),
  };
  const labels = lang === 'en'
    ? { dev:'dev mode', build:'production build', install:'install' }
    : { dev:'开发模式', build:'生产构建', install:'安装包' };

  return (
    <section id="start">
      <div className="section-head">
        <span className="eyebrow">Quick Start · 快速开始</span>
        <h2>{lang === 'en' ? 'Up in two minutes.' : '两分钟跑起来。'}</h2>
        <p>{lang === 'en'
          ? 'Build from source needs Node 18+, pnpm 9+, Rust 1.77+.'
          : '源码构建需要 Node 18+、pnpm 9+、Rust 1.77+。'}</p>
      </div>
      <div className="tabs" role="tablist">
        {Object.entries(labels).map(([k,v]) => (
          <button key={k} className={`tab ${tab===k?'active':''}`} onClick={() => setTab(k)} role="tab">{v}</button>
        ))}
      </div>
      {blocks[tab]}
    </section>
  );
}

function FAQ({ lang }) {
  const [open, setOpen] = useState(0);
  const items = lang === 'en' ? [
    { q:'Why another terminal?', a:'Native terminals are great. Ridge isn\'t trying to replace them — it stitches the terminal, editor and git graph together so an agent can drive multiple panes at once.' },
    { q:'Is the editor a Monaco or a CodeMirror fork?', a:<>Ridge embeds a CodeMirror 6 instance in each editor pane. Settings sync across all panes; the active language server is reused.</> },
    { q:'How do agents talk to panes?', a:<>Through Claude Code\'s session protocol. Agents can <code>list</code>, <code>name</code>, <code>create</code> and <code>close</code> panes, and query each pane\'s working directory.</> },
    { q:'Does it phone home?', a:'No. There are no analytics, no telemetry, no auto-update pings. The repo is MIT — audit it yourself.' },
    { q:'Why Tauri instead of Electron?', a:'A 18 MB binary instead of 200 MB+, native webviews, and a Rust core that owns the PTY layer. The shell experience matters; we don\'t want a Node bottleneck in front of every keystroke.' },
    { q:'Where do macOS / Linux builds live?', a:'v0.1.0 ships only Windows binaries. macOS and Linux build cleanly from source today; signed builds land in v0.2.' },
  ] : [
    { q:'为什么再做一个终端？', a:'原生终端已经很好了。Ridge 不是想取代它们——而是把终端、编辑器、git 图缝在一起，让智能体能同时驱动多个分屏。' },
    { q:'编辑器是 Monaco 还是 CodeMirror 的 fork？', a:<>Ridge 在每个编辑器分屏里嵌入了 CodeMirror 6。所有分屏共享配置；语言服务可被复用。</> },
    { q:'智能体是如何和分屏通信的？', a:<>通过 Claude Code 的会话协议：智能体可以执行 <code>list</code>、<code>name</code>、<code>create</code>、<code>close</code>，也能查询每个分屏的工作目录。</> },
    { q:'会上报数据吗？', a:'不会。没有分析、没有 telemetry、没有自动更新心跳。仓库 MIT 协议，可自行审计。' },
    { q:'为什么选 Tauri 而不是 Electron？', a:'18 MB 体积对比 200 MB+，原生 webview，Rust 内核直接管理 PTY 层。终端体验很重要——我们不希望每次按键都经过一个 Node 中转。' },
    { q:'macOS / Linux 的安装包在哪里？', a:'v0.1.0 仅提供 Windows 二进制。macOS 与 Linux 当下可以源码自构建；签名版本会在 v0.2 跟上。' },
  ];

  return (
    <section id="faq">
      <div className="section-head">
        <span className="eyebrow">FAQ · 常见问题</span>
        <h2>{lang === 'en' ? 'Things people ask.' : '常被问到的问题。'}</h2>
      </div>
      <div className="faq-list">
        {items.map((it, i) => (
          <div className={`faq-item ${open===i?'open':''}`} key={i}>
            <button className="faq-q" onClick={() => setOpen(open===i ? -1 : i)}>
              <span>{it.q}</span>
              <span className="faq-mark">+</span>
            </button>
            <div className="faq-a">
              <div className="faq-a-inner">{it.a}</div>
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

function ReleasePeek({ lang }) {
  return (
    <section id="latest">
      <div className="section-head">
        <span className="eyebrow">Latest · v0.1.0</span>
        <h2>{lang === 'en' ? 'Latest release · v0.1.0' : '最新版本 · v0.1.0'}</h2>
        <p>{lang === 'en'
          ? 'Ridge 0.1.0 is the first public release. Recursive splits, embedded editor, git graph and agent collab all ship together.'
          : 'Ridge 0.1.0 是首个对外发布的版本，奠定了分屏、终端、编辑器、Git 与智能体协作的基础体验。'}</p>
      </div>
      <div className="release">
        <div className="release-head">
          <div className="release-tag"><span className="v">v</span>0.1.0</div>
          <div className="release-meta">
            <span className="pill">FIRST RELEASE</span>
            <span>2026-04-30</span>
          </div>
        </div>
        <p>{lang === 'en'
          ? 'The first public version. Split panes, terminals, editor, git graph and agent collaboration all become usable in the same window for the first time.'
          : 'Ridge 的第一个公开版本。在这一版里，分屏终端、代码编辑器、Git 提交图与智能体协作首次同时可用。'}</p>
        <h4>{lang === 'en' ? 'Highlights' : '亮点'}</h4>
        <ul>
          {(lang === 'en' ? [
            'Recursive splits + multi-workspace',
            'Stable terminal w/ MB-scale scrollback',
            'Embedded editor sharing the split layout',
            'Git commit graph + live repo state',
            'Claude Code multi-pane agent collab out of the box',
            'Multiple themes + switchable editor fonts',
          ] : [
            '递归分屏与多工作区',
            '稳定的终端体验，可滚动数 MB 的命令历史',
            '内置代码编辑器，与终端共享分屏布局',
            'Git 提交图与实时仓库状态',
            '开箱即用的 Claude Code 智能体协作',
            '多套主题、可切换编辑器字体',
          ]).map((li, i) => <li key={i}>{li}</li>)}
        </ul>
        <p style={{marginTop:18}}>
          <a href="https://github.com/MySetsuna/ridge/releases" className="btn btn-sm" target="_blank" rel="noopener">{lang === 'en' ? 'See all releases →' : '查看全部 Release →'}</a>
        </p>
      </div>
    </section>
  );
}

function Foot({ lang }) {
  return (
    <footer className="foot">
      <div className="foot-inner">
        <div>
          <strong style={{color:'var(--crop)'}}>Ridge</strong> · MIT License · 田埂
          <div style={{color:'var(--mist-soft)', fontSize:12, fontFamily:'var(--font-mono)', marginTop:6}}>
            Built with Tauri 2 · Svelte 5 · Rust · TypeScript
          </div>
        </div>
        <div className="links">
          <a href="https://github.com/MySetsuna/ridge" target="_blank" rel="noopener">GitHub</a>
          <a href="#features">{lang === 'en' ? 'Features' : '特性'}</a>
          <a href="#start">{lang === 'en' ? 'Quick start' : '快速开始'}</a>
          <a href="https://github.com/MySetsuna/ridge/issues" target="_blank" rel="noopener">Issues</a>
        </div>
      </div>
    </footer>
  );
}

window.Nav = Nav;
window.Hero = Hero;
window.Features = Features;
window.Philosophy = Philosophy;
window.Showcase = Showcase;
window.Keyboard = Keyboard;
window.Compare = Compare;
window.QuickStart = QuickStart;
window.FAQ = FAQ;
window.ReleasePeek = ReleasePeek;
window.Foot = Foot;
