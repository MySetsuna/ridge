/**
 * markdown.test.ts — renderer stamps source-line attributes on block elements,
 * generates stable heading slugs, and toggleTaskAtLine mutates the source
 * exactly at the given line.
 *
 * monaco-editor is imported transitively but only `colorize` is used and only
 * inside `highlightCodeBlocks` (which we don't exercise here). We mock the
 * module to avoid loading Monaco's JSDOM-incompatible bundle in Node.
 */
import { describe, it, expect, vi } from 'vitest';

vi.mock('monaco-editor', () => ({
  editor: {
    colorize: vi.fn(async (text: string) => text),
  },
}));

// Import after mock is in place.
const { renderMarkdown, toggleTaskAtLine, isMarkdownPath, stripFrontMatter } = await import(
  './markdown'
);

describe('renderMarkdown — source-line stamping', () => {
  it('stamps data-wf-md-src-line on paragraphs in source order', () => {
    const src = 'first\n\nsecond\n\nthird';
    const html = renderMarkdown(src);
    const matches = [...html.matchAll(/<p data-wf-md-src-line="(\d+)">(.+?)<\/p>/g)];
    expect(matches).toHaveLength(3);
    const lines = matches.map((m) => Number(m[1]));
    // Each paragraph starts at a distinct line index; monotonically non-decreasing.
    expect(lines[0]).toBeLessThan(lines[1]);
    expect(lines[1]).toBeLessThan(lines[2]);
  });

  it('stamps headings with id (slug) and src line', () => {
    const src = '# Hello World\n\n## Sub Heading';
    const html = renderMarkdown(src);
    expect(html).toMatch(/<h1 id="hello-world" data-wf-md-src-line="0">/);
    expect(html).toMatch(/<h2 id="sub-heading" data-wf-md-src-line="2">/);
  });

  it('disambiguates duplicate heading slugs with counter suffix', () => {
    const src = '# Title\n\nsome\n\n# Title\n\nagain';
    const html = renderMarkdown(src);
    const ids = [...html.matchAll(/<h1 id="([^"]+)"/g)].map((m) => m[1]);
    expect(ids).toEqual(['title', 'title-1']);
  });

  it('stamps fenced code blocks', () => {
    const src = '```js\nconsole.log(1);\n```';
    const html = renderMarkdown(src);
    expect(html).toMatch(/<pre[^>]*data-wf-md-src-line="0"/);
  });

  it('stamps blockquotes and lists', () => {
    const src = '> quote\n\n- item a\n- item b';
    const html = renderMarkdown(src);
    expect(html).toMatch(/<blockquote data-wf-md-src-line="\d+">/);
    expect(html).toMatch(/<ul data-wf-md-src-line="\d+">/);
  });

  it('handles duplicate paragraph text without mismapping lines', () => {
    // Two identical "hi" paragraphs should get two distinct src lines —
    // the consumed-line FIFO counter makes this deterministic.
    const src = 'hi\n\nhi\n\nhi';
    const html = renderMarkdown(src);
    const lines = [...html.matchAll(/<p data-wf-md-src-line="(\d+)">/g)].map((m) =>
      Number(m[1])
    );
    // Three separate lines, strictly increasing.
    expect(lines).toHaveLength(3);
    expect(new Set(lines).size).toBe(3);
  });
});

describe('toggleTaskAtLine', () => {
  const source = [
    '# Title',
    '',
    '- [ ] first',
    '- [x] second',
    'plain line',
  ].join('\n');

  it('toggles unchecked → checked', () => {
    const out = toggleTaskAtLine(source, 2);
    expect(out.split('\n')[2]).toBe('- [x] first');
  });

  it('toggles checked → unchecked (preserves other lines exactly)', () => {
    const out = toggleTaskAtLine(source, 3);
    const lines = out.split('\n');
    expect(lines[3]).toBe('- [ ] second');
    expect(lines[0]).toBe('# Title');
    expect(lines[4]).toBe('plain line');
  });

  it('returns source unchanged for a non-task line', () => {
    expect(toggleTaskAtLine(source, 4)).toBe(source);
  });

  it('returns source unchanged for negative / OOB indices', () => {
    expect(toggleTaskAtLine(source, -1)).toBe(source);
    expect(toggleTaskAtLine(source, 9999)).toBe(source);
  });
});

describe('renderMarkdown — image renderer (lazy load)', () => {
  it('emits loading="lazy" and decoding="async" for images', () => {
    const html = renderMarkdown('![alt text](src.png)');
    expect(html).toContain('loading="lazy"');
    expect(html).toContain('decoding="async"');
    expect(html).toContain('src="src.png"');
    expect(html).toContain('alt="alt text"');
  });

  it('escapes HTML-sensitive characters in alt text', () => {
    // The alt contains a literal `&`, `<`, `>`, `"` — must be entity-encoded
    // so the attribute value remains well-formed.
    const html = renderMarkdown('![A & "B" <C>](pic.png)');
    expect(html).toMatch(/alt="A &amp; &quot;B&quot; &lt;C&gt;"/);
    // No raw unescaped quotes leaked into the alt attribute.
    expect(html).not.toMatch(/alt="A & "B"/);
  });

  it('preserves and escapes the title attribute when present', () => {
    const html = renderMarkdown('![logo](logo.svg "Brand & Co")');
    expect(html).toMatch(/title="Brand &amp; Co"/);
  });
});

describe('stripFrontMatter', () => {
  it('hides a YAML front-matter block at the very top', () => {
    const src = '---\ntitle: Foo\ndate: 2024-01-01\n---\n\n# Real content\n';
    const html = renderMarkdown(src);
    // Real content is rendered…
    expect(html).toMatch(/<h1[^>]*>Real content<\/h1>/);
    // …and the YAML keys are NOT visible in the output.
    expect(html).not.toContain('title: Foo');
    expect(html).not.toContain('date: 2024-01-01');
    // No stray <hr/> from the closing ---.
    expect(html).not.toMatch(/<hr\s*\/?>/);
  });

  it('hides a TOML (+++) front-matter block at the very top', () => {
    const src = '+++\ntitle = "Foo"\n+++\n\n# Real content\n';
    const html = renderMarkdown(src);
    expect(html).toMatch(/<h1[^>]*>Real content<\/h1>/);
    expect(html).not.toContain('title = "Foo"');
  });

  it('preserves a mid-document --- thematic break', () => {
    const src = 'before\n\n---\n\nafter';
    const html = renderMarkdown(src);
    // Marked emits <hr> for the thematic break.
    expect(html).toMatch(/<hr\s*\/?>/);
    expect(html).toContain('before');
    expect(html).toContain('after');
  });

  it('does not strip front-matter that is not on the very first line', () => {
    const src = '\n---\ntitle: Foo\n---\n\n# After';
    // Leading blank line → first line is empty, NOT `---`. So `---` here is
    // a thematic break / setext heading marker; either way, it should not be
    // treated as front-matter.
    const out = stripFrontMatter(src);
    expect(out).toBe(src);
  });

  it('does not strip when the closing fence is missing', () => {
    const src = '---\ntitle: Foo\nno closing fence here\n\n# Body';
    const out = stripFrontMatter(src);
    expect(out).toBe(src);
  });

  it('keeps source line numbers stable downstream (replaces with blank lines)', () => {
    // Front-matter occupies lines 0..3 (4 lines incl. closing ---).
    // Lines 4 (blank) and 5 (paragraph "hello") follow.
    // The paragraph's data-wf-md-src-line should still be 5, not 1.
    const src = '---\ntitle: Foo\nx: 1\n---\n\nhello';
    const html = renderMarkdown(src);
    const m = html.match(/<p data-wf-md-src-line="(\d+)">hello<\/p>/);
    expect(m).not.toBeNull();
    expect(Number(m?.[1])).toBe(5);
  });

  it('strips YAML front-matter with CRLF line endings', () => {
    // Windows core.autocrlf=true produces \r\n; the fence check must not fail
    // on `'---\r'` vs `'---'`.
    const src = '---\r\ntitle: Foo\r\ndate: 2024-01-01\r\n---\r\n\r\n# Real content\r\n';
    const html = renderMarkdown(src);
    expect(html).toMatch(/<h1[^>]*>Real content<\/h1>/);
    expect(html).not.toContain('title: Foo');
    expect(html).not.toContain('date: 2024-01-01');
  });

  it('strips JSON front-matter delimited by { / }', () => {
    // Some SSGs (Hexo, old Hugo) use a lone `{` / `}` as the delimiter.
    const src = '{\n"title": "Foo",\n"date": "2024-01-01"\n}\n\n# Real content\n';
    const html = renderMarkdown(src);
    expect(html).toMatch(/<h1[^>]*>Real content<\/h1>/);
    expect(html).not.toContain('"title"');
    expect(html).not.toContain('"date"');
  });

  it('does NOT strip a { that is not on the very first line', () => {
    // A `{` alone in the middle of the document (e.g. code example) must
    // never be treated as front-matter.
    const src = 'intro\n\n{\n"key": "value"\n}\n\nafter';
    const out = stripFrontMatter(src);
    expect(out).toBe(src);
  });
});

describe('isMarkdownPath', () => {
  it.each([
    ['README.md', true],
    ['notes.markdown', true],
    ['a/b/c.mdown', true],
    ['Capitals.MD', true],
    ['.md', true],
    ['notes.txt', false],
    ['README', false],
    ['file.md.bak', false],
  ])('%s → %s', (path, expected) => {
    expect(isMarkdownPath(path)).toBe(expected);
  });
});
