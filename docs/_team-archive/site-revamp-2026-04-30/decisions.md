# Team-lead decisions on ui-designer's open questions

Reference: `site/_brief/content-brief.md`

| # | Question | Decision |
|---|---|---|
| 1 | Brand touch placement | **Only `docs.html` About section** (one sentence). Drop from README. |
| 2 | `<title>` wording | `Ridge — 分屏终端工作台` (designer's default — clean, accurate, concise) |
| 3 | Showcase eyebrow style | **`01 · Split` / `02 · Editor` / `03 · Git` / `04 · Agents`** — minimal numbered, drop "Scene"/"Plot" entirely |
| 4 | "首页发布段" h2 | `最新版本 v0.1.0` (block is a peek of the latest release; lead with the number) |
| 5 | Footer | `Ridge · MIT License` (concise, drop tagline duplication) |
| 6 | "Built with" footnote | **`Tauri 2 · Svelte 5 · Rust · TypeScript`** — include majors for accuracy |
| 7 | 404 page "4 0 4" decoration | **Keep**, only neutralise body copy |

## Additional standing rules for the editor

- Do not introduce new sections or rearrange page-level structure beyond what the brief specifies.
- Do not touch CSS/JS — only HTML content edits + plain-text Markdown rewrites in CHANGELOG.md and README.md.
- Preserve all existing CSS class names, ids, and link targets so styles and anchors keep working.
- Do not remove or rename the file paths under `site/assets/` — placeholders + media slot system stays as is.
- All emoji-free.
- Keep the Chinese-primary tone; English where the original was English.

## Brand-touch one-liner (canonical text)

The brief permits a single light brand-name explanation. Use **exactly this** sentence in `docs.html` About section, and nowhere else:

> Ridge 取自田埂——把屏幕分割成可以独立工作的几块。

(One sentence, declarative, no metaphor scaffolding around it.)
