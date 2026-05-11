/* global React */
const { useState, useEffect, useRef } = React;

/* ============ Animated Workbench (hero) ============ */
function Workbench() {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick(t => t + 1), 1500);
    return () => clearInterval(id);
  }, []);
  const [typed, setTyped] = useState('');
  const target = '$ pnpm tauri dev';
  useEffect(() => {
    let i = 0;
    const id = setInterval(() => {
      i++;
      setTyped(target.slice(0, i));
      if (i >= target.length) clearInterval(id);
    }, 80);
    return () => clearInterval(id);
  }, [Math.floor(tick / 6)]);

  const aheadCount = 1 + (tick % 4);

  return (
    <div className="workbench">
      <div className="wb-titlebar">
        <div className="lights"><span></span><span></span><span></span></div>
        <div className="center">
          <span>~/code/ridge</span>
          <span className="branch">⎇ feat/agent-team</span>
          <span>· 4 panes</span>
        </div>
        <div className="right">
          <span style={{color:'var(--mist-soft)'}}>v0.1.0</span>
          <span className="pulse-dot"></span>
        </div>
      </div>
      <div className="wb-grid">
        {/* Terminal — animated typing */}
        <div className="pane active" style={{gridRow:'1', gridColumn:'1'}}>
          <div className="pane-head">
            <span className="label">▎ pane · terminal</span>
            <span className="meta">~/ridge · zsh</span>
          </div>
          <div className="pane-body">
            <div className="row dim"># ridge is the workbench you're inside.</div>
            <div className="row cmd">{typed}<span className="cursor"></span></div>
            <div className="row dim">{typed.length >= target.length ? '▸ vite v6.0 ready in 412ms' : ''}</div>
            <div className="row ok">{typed.length >= target.length ? '▸ tauri compiled OK' : ''}</div>
            <div className="row dim">{typed.length >= target.length ? '▸ listening on 127.0.0.1:1420' : ''}</div>
          </div>
        </div>

        {/* Git graph — animated commits */}
        <div className="pane" style={{gridRow:'1', gridColumn:'2'}}>
          <div className="pane-head">
            <span className="label">▎ pane · git</span>
            <span className="meta">{aheadCount} ahead</span>
          </div>
          <div className="pane-body" style={{paddingLeft: 14}}>
            <GitGraphMini tick={tick} aheadCount={aheadCount} />
          </div>
        </div>

        {/* Editor */}
        <div className="pane" style={{gridRow:'2', gridColumn:'1'}}>
          <div className="pane-head">
            <span className="label">▎ pane · editor</span>
            <span className="meta">src/pane.rs · M</span>
          </div>
          <div className="pane-body">
            <div className="row"><span className="dim">12</span> <span className="ridge">pub fn</span> <span className="cmd">render</span>(&amp;<span className="soil">self</span>) {'{'}</div>
            <div className="row"><span className="dim">13</span>   <span className="ridge">let</span> <span className="cmd">plot</span> = <span className="soil">self</span>.split(<span style={{color:'var(--sun)'}}>Axis::Vert</span>);</div>
            <div className="row"><span className="dim">14</span>   plot.attach(<span className="soil">self</span>.shell);</div>
            <div className="row"><span className="dim">15</span>   <span className="ridge">Ok</span>(plot)</div>
            <div className="row"><span className="dim">16</span> {'}'}</div>
          </div>
        </div>

        {/* Agent */}
        <div className="pane" style={{gridRow:'2', gridColumn:'2'}}>
          <div className="pane-head">
            <span className="label">▎ pane · agent</span>
            <span className="meta">claude-code</span>
          </div>
          <div className="pane-body">
            <div className="row soil">claude › <span style={{color:'var(--ridge)'}}>✦</span></div>
            <div className="row dim">scaffolding 4 modules…</div>
            <div className="row ok">✓ wrote src/pane.rs</div>
            <div className="row ok">✓ wrote src/split.rs</div>
            <div className="row warn">⏵ awaiting approval</div>
          </div>
        </div>
      </div>
    </div>
  );
}

function GitGraphMini({ tick, aheadCount }) {
  // simple two-branch graph
  const nodes = [
    { y: 14, x: 8, c: 'var(--mist)', l: 'init' },
    { y: 34, x: 8, c: 'var(--mist)', l: 'split-pane core' },
    { y: 54, x: 8, c: 'var(--ridge)', l: 'main' },
    { y: 78, x: 22, c: 'var(--soil)', l: 'feat/agent' },
    { y: 100, x: 22, c: 'var(--soil)', l: 'pane proto' },
    { y: 122, x: 22, c: aheadCount > 2 ? 'var(--soil-bright)' : 'var(--soil)', l: 'team mode' },
  ];
  return (
    <svg viewBox="0 0 220 140" style={{width:'100%', height:'100%'}}>
      {/* trunk */}
      <line x1="8" y1="8" x2="8" y2="60" stroke="var(--line)" strokeWidth="1.5" />
      <line x1="8" y1="60" x2="22" y2="78" stroke="var(--ridge-deep)" strokeWidth="1.5" />
      <line x1="22" y1="78" x2="22" y2="128" stroke="var(--soil)" strokeWidth="1.5" strokeDasharray={tick % 2 ? '0' : '0'} />
      {/* nodes */}
      {nodes.map((n, i) => (
        <g key={i}>
          <circle cx={n.x} cy={n.y} r="3.5" fill={n.c} />
          <text x={n.x + 10} y={n.y + 3.5} fill="var(--crop-soft)" fontSize="9" fontFamily="var(--font-mono)">
            {n.l}
          </text>
        </g>
      ))}
      {/* HEAD label */}
      <g>
        <rect x="120" y="116" width="46" height="14" rx="3" fill="rgba(217,119,87,0.15)" stroke="var(--soil)" />
        <text x="143" y="125.5" textAnchor="middle" fill="var(--soil-bright)" fontSize="8" fontFamily="var(--font-mono)">HEAD</text>
      </g>
    </svg>
  );
}

/* ============ Tian glyph (philosophy) ============ */
function TianGlyph() {
  const [hover, setHover] = useState(null);
  const plots = [
    { id: 'tl', x: 0, y: 0, fill: 'rgba(127,176,105,0.18)', label: 'TERMINAL' },
    { id: 'tr', x: 1, y: 0, fill: 'rgba(217,119,87,0.05)', label: 'GIT' },
    { id: 'bl', x: 0, y: 1, fill: 'rgba(127,176,105,0.06)', label: 'EDITOR' },
    { id: 'br', x: 1, y: 1, fill: 'rgba(217,119,87,0.18)', label: 'AGENT' },
  ];
  return (
    <svg viewBox="0 0 200 200" className="tian-svg">
      <defs>
        <pattern id="paddy" width="8" height="8" patternUnits="userSpaceOnUse">
          <path d="M 8 0 L 0 0 0 8" fill="none" stroke="var(--line-soft)" strokeWidth="0.5" />
        </pattern>
      </defs>
      {/* outer */}
      <rect x="20" y="20" width="160" height="160" fill="url(#paddy)" stroke="var(--line)" strokeWidth="1.5" />
      {/* plots fill */}
      {plots.map(p => (
        <rect key={p.id}
          x={20 + p.x * 80}
          y={20 + p.y * 80}
          width="80" height="80"
          fill={hover === p.id ? 'rgba(127,176,105,0.28)' : p.fill}
          onMouseEnter={() => setHover(p.id)}
          onMouseLeave={() => setHover(null)}
          style={{cursor:'pointer', transition:'fill 0.2s ease'}}
        />
      ))}
      {/* ridge cross — the 田埂 itself */}
      <line x1="100" y1="20" x2="100" y2="180" stroke="var(--ridge)" strokeWidth="2.5" />
      <line x1="20" y1="100" x2="180" y2="100" stroke="var(--ridge)" strokeWidth="2.5" />
      {/* outer frame */}
      <rect x="20" y="20" width="160" height="160" fill="none" stroke="var(--ridge)" strokeWidth="2.5" />
      {/* labels on hover */}
      {plots.map(p => (
        <text key={p.id + 'l'}
          x={20 + p.x * 80 + 40}
          y={20 + p.y * 80 + 44}
          textAnchor="middle"
          fontFamily="var(--font-mono)"
          fontSize="9"
          fill={hover === p.id ? 'var(--ridge-bright)' : 'var(--mist-soft)'}
          letterSpacing="2"
          style={{pointerEvents:'none', transition:'fill 0.2s ease'}}
        >{p.label}</text>
      ))}
      {/* corner mono labels */}
      <text x="22" y="14" fontFamily="var(--font-mono)" fontSize="8" fill="var(--mist-soft)" letterSpacing="1">FIELD · 田</text>
      <text x="178" y="194" textAnchor="end" fontFamily="var(--font-mono)" fontSize="8" fill="var(--mist-soft)" letterSpacing="1">RIDGE · 埂</text>
    </svg>
  );
}

window.Workbench = Workbench;
window.TianGlyph = TianGlyph;
