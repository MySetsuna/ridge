// Ridge — site interactions. Tiny, no framework.
(() => {
  // ─── Tabs (Quick Start) ────────────────────────────────
  const tabs = document.querySelectorAll('.tab');
  const panels = document.querySelectorAll('.tab-panel');
  tabs.forEach((tab) => {
    tab.addEventListener('click', () => {
      const target = tab.dataset.tab;
      tabs.forEach((t) => {
        const active = t === tab;
        t.classList.toggle('active', active);
        t.setAttribute('aria-selected', active ? 'true' : 'false');
      });
      panels.forEach((p) => p.classList.toggle('active', p.dataset.tab === target));
    });
  });

  // ─── Reveal-on-scroll ──────────────────────────────────
  if ('IntersectionObserver' in window && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    const io = new IntersectionObserver((entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add('in');
          io.unobserve(entry.target);
        }
      });
    }, { rootMargin: '0px 0px -8% 0px', threshold: 0.05 });
    document.querySelectorAll('.reveal').forEach((el) => io.observe(el));
  } else {
    document.querySelectorAll('.reveal').forEach((el) => el.classList.add('in'));
  }

  // ─── Hash-based active nav highlight ───────────────────
  const navLinks = document.querySelectorAll('.nav-links a[href^="#"]');
  const sections = [...navLinks]
    .map((a) => document.querySelector(a.getAttribute('href')))
    .filter(Boolean);
  if (sections.length && 'IntersectionObserver' in window) {
    const setActive = (id) => {
      navLinks.forEach((a) => a.classList.toggle('active', a.getAttribute('href') === '#' + id));
    };
    const navIO = new IntersectionObserver(
      (entries) => {
        entries.forEach((e) => {
          if (e.isIntersecting) setActive(e.target.id);
        });
      },
      { rootMargin: '-40% 0px -55% 0px', threshold: 0 }
    );
    sections.forEach((s) => navIO.observe(s));
  }

  // ─── Auto-promote to <video> when MP4 placed ───────────
  // If site/assets/media/<name>.mp4 exists, swap the placeholder <img>
  // for an autoplaying muted loop. Probes via HEAD request — fails silently.
  document.querySelectorAll('.media-body img[src*="placeholders/"]').forEach((img) => {
    const tag = img.parentElement.querySelector('.placeholder-tag');
    if (!tag) return;
    const m = tag.textContent.match(/replace:\s*(\S+)/);
    if (!m) return;
    const realPath = './' + m[1].trim();
    if (!/\.mp4$/i.test(realPath)) {
      // For non-mp4 (gif/png), just probe and swap <img src>.
      const probe = new Image();
      probe.onload = () => { img.src = realPath; tag.remove(); };
      probe.src = realPath;
      return;
    }
    fetch(realPath, { method: 'HEAD' }).then((r) => {
      if (!r.ok) return;
      const v = document.createElement('video');
      v.src = realPath;
      v.autoplay = true;
      v.muted = true;
      v.loop = true;
      v.playsInline = true;
      v.setAttribute('aria-label', 'Demo recording');
      img.replaceWith(v);
      tag.remove();
    }).catch(() => {});
  });

  // ─── Year footer ───────────────────────────────────────
  const y = document.querySelector('[data-year]');
  if (y) y.textContent = new Date().getFullYear();
})();
