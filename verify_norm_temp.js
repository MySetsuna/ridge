const { marked } = require('marked');

function normaliseWindowsPathLinks(source) {
  const parts = source.split(/(`+[^`]*`+)/g);
  const SCHEME_RE = /^[a-zA-Z][a-zA-Z0-9+.-]*:/;
  for (let i = 0; i < parts.length; i += 2) {
    parts[i] = parts[i].replace(/(\]\()([^)\s][^)]*)(\))/g, (full, open, target, close) => {
      const trimmed = target.trim();
      if (SCHEME_RE.test(trimmed) || trimmed.startsWith('//') || trimmed.startsWith('#')) {
        return full;
      }
      if (!target.includes('\\')) return full;
      return open + target.replace(/\\/g, '/') + close;
    });
  }
  return parts.join('');
}

const cases = [
  '[A](docs\\sub\\file.md)',
  '[B](C:\\Users\\me\\file.md)',
  '[C](https://example.com/foo)',
  'inline `back\\slash` outside [link](rel\\path.md)',
  '[D](mailto:foo@bar.com)',
  '[E](#section)',
  '[F](./normal.md)',
];
for (const c of cases) {
  const norm = normaliseWindowsPathLinks(c);
  const html = marked.parse(norm, { gfm: true });
  console.log('IN :', JSON.stringify(c));
  console.log('OUT:', JSON.stringify(norm));
  console.log('HTML:', html.replace(/\n+/g, ' '));
  console.log();
}
