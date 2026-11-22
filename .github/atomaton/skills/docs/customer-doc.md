---
name: docs/customer-doc
description: Load when a run must deliver a document a customer will open — a docx report, a pptx deck or an xlsx estimate — by filling a template from a data file, rendering every page to an image and reading those images until the layout is right, then reporting what was produced and what a person still has to check.
---

# Producing a customer document: template, fill, then look at it

A document that leaves the building is judged by how it looks, not by whether the
file opened. So this procedure has two halves that cannot be separated: a **fill**
that puts the run's data into a template, and a **visual loop** that renders the
result and reads every page of it. A run that fills and does not look has not
finished the job; it has produced a file and does not know what is in it.

The scripts, both beside this file in the shipped tree:

| Script | What it is for |
| --- | --- |
| `customer-doc.mjs` | `deps`, `template`, `fill`, `render`, `uno` — this procedure |
| `deps.mjs` | the LibreOffice, CJK-font and venv install both legs below use |

Dependencies are installed on demand into the cache directory and never into
`environment.setup_commands`. `bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" deps` installs and reports
everything the other subcommands use, so a first run on a machine pays once:
measured 9–39 s for LibreOffice (a 213 MB tarball, 15 `.deb`s extracted, 584 MB
unpacked, the difference being the download), 3.4 s for the CJK fonts, 2.4 s for the venv, and under a second each
for the two pip packages. Every later run says `cached` for all of them and takes
under a second.

## 1. Which leg — decided by the template, not by preference

| Template | Leg | What it means |
| --- | --- | --- |
| `.fodt` / `.fodp` / `.fods`, or the name `docx`/`pptx`/`xlsx` | **flat ODF** | the placeholders are substituted in XML text, then converted to the binary format by LibreOffice |
| any other path — a customer's `.docx`, `.pptx`, `.xlsx` | **UNO** | a headless LibreOffice opens the template, replaces the placeholders in the document model, and stores it back |

```bash
bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" fill --template docx --data data.json --out out/report.docx
bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" fill --template /path/to/customer-form.docx --data data.json --out out/form.docx
bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" render out/report.docx --outdir out/render --dpi 120
```

The full path matters: the scripts are not in a run's working directory, so every
example spells the shipped location — `${ATOMATON_MACHINERY_ROOT:-.}` is where the
run's copy of the machinery lives, with the checkout as the `:-.` fallback.

Prefer the flat leg. Its source is text, so a fix is an edit in a diff and one
more conversion — not a round trip through a binary that rewrites everything it
touches. The UNO leg exists for the template you were handed, and for that one it
is the only leg there is.

## 2. The templates and the placeholder contract

Three defaults ship under `templates/`, each a flat ODF file with a comment header
that states what was measured about it:

| File | Format | Shape |
| --- | --- | --- |
| `report.fodt` | docx | A4 portrait; title page with a three-row metadata table; five numbered sections, the first starting a new page; three two-column tables with a repeating header row; footer with organisation, title and page numbers |
| `deck.fodp` | pptx | 16:9; three slides: title with subtitle and one metadata line, agenda with four numbered items, content with a heading, four bullets and a note |
| `book.fods` | xlsx | two sheets: an itemised quotation (header block, four line items, formula subtotal / tax / grand total, terms) and an assumptions sheet |

**The defaults hold structure and formatting only; the language comes from the data.**
Every word a reader sees is a placeholder — section headings, metadata labels,
column headers, the footer's organisation name, the tax label, the currency in the
number format and the sheet names included — so the same template sets a Japanese
document and an English one, and a language nobody has used yet, from the data
alone. What deliberately stays in the template is structure and typography: the
ordinal in a heading (`1.`), an agenda's `01`..`04`, the bullet glyph, an ASCII
separator between two values, and the fact that a label placeholder carries its own
punctuation with it, so `件名：` and `Subject: ` are both data rather than one
template with a colon glued on.

**The font is `Noto Sans CJK JP` on every style, and that is not a language
constraint.** It is a multi-script coverage choice: one family carries Latin and CJK,
so the same template sets data in either without naming a second font. Keeping it
also keeps §3's first trap at bay — leave the East Asian face unstated and
LibreOffice derives one from the document language, landing on a Simplified-Chinese
serif whose Japanese glyphs are the wrong shapes.

**A template a person attached to the issue.** The customer's own template usually
arrives as a file on the issue rather than as a path in the tree. `fetch_attachment`
turns it into one: pass the URL from the comment it was posted in
(`https://github.com/user-attachments/files/<id>/<name>`), and the bytes are saved
under `/tmp/atomaton-workspace/attachments` — the one `/tmp` directory that survives
into the next run on the issue. It reads only GitHub's two attachment hosts and sends
no credential, so an attachment on a private repository comes back refused rather than
saved — as a 404 or as a page, which of the two has not been measured. That saved path
is then what `--template` takes in §4: a customer's `.docx` is the UNO leg.

**A template too big to attach.** GitHub caps a chat attachment at 25 MB, so a
heavier template cannot be attached at all. Ask the person to compress it first —
Office's built-in picture compression (図の圧縮) usually brings a media-heavy
template down by an order of magnitude — and re-attach. A template that will not
fit even compressed goes into the tree by git, and `--template` takes that path
directly.

A placeholder is `{{UPPER_SNAKE}}` and is substituted everywhere it appears,
including inside table cells. Values are XML-escaped, so a name with `&` or `<`
in it is safe. The data file is one JSON object, one key per placeholder.

Three properties of a *spreadsheet* template are not free choices, each measured
by filling and re-rendering:

- a numeric placeholder must be written into **both** `office:value` and the cell
  text. The value is what the format carries as a number and what a formula
  reads; the text is what the cell displays. Text alone makes it a text cell —
  no number format, no right alignment, and a formula pointing at it computes
  nothing;
- a **formula** cell must not carry a cached `office:value`: LibreOffice trusts a
  cached value and never recalculates, so every total came back 0 until the
  attribute was removed;
- `style:page-layout` must live in `office:automatic-styles`. Placed in
  `office:styles` it is not bound to the master page — the export carries
  `style:page-layout-name=""` and LibreOffice prints with its own default margins,
  which split a 16.5 cm table across two pages instead of one.

`fill` reports, in its JSON line, the three mistakes worth knowing about:

| Field | Meaning | What to do |
| --- | --- | --- |
| `keysNotInTemplate` | data keys no placeholder matched | the data or the template is wrong; do not ship |
| `leftInTemplate` | placeholders still in the output | a typo in the key, or the placeholder is not plain text |
| `unfilled` | keys the fill could not place | same as above, and the reason this field exists |

## 3. The flat leg, and the rules that cost a render each

Substituting text is trivial; making the *conversion* keep the document's
intent is not. These rules were each found by rendering a draft that looked fine in
the XML, and they are kept as measurements, not as an assumption about language:
they apply whenever the data that flows through a template hits the trap.

- **State the East Asian font, size and language on every text style.** Without
  `style:font-name-asian` the exported docx carries
  `w:eastAsia="Noto Serif CJK SC"` — LibreOffice's default Asian face, a
  Simplified-Chinese serif — instead of the Noto Sans JP the template asked for;
  without `style:language-asian="ja"` it carries `w:lang w:eastAsia="en-US"`, so
  the reader treats Japanese as English for line breaking and fallback. The
  second defect is invisible in a render and only shows in the XML, which is the
  argument for checking both.
- **docx:** a heading needs `fo:keep-with-next="always"`, or it strands at the
  foot of a page.
- **docx, a table across a page break:** what stops a row splitting is a row
  property, not the table's. `style:may-break-between-rows="false"` on the table
  decides only whether a table that would fit moves whole — measured on a 6-row
  table with 30 filler paragraphs above it, without it rows 1–3 stayed on page 1
  and 4–6 went to page 2, and with it all six moved to page 2. It exports as
  `w:keepNext` on the row paragraphs and never as `w:cantSplit`, so on a table
  too long for one page it decides nothing: 150 rows with it alone split across 3
  pages at a data row, with 0 `w:cantSplit`, 0 `w:tblHeader`, and the header row
  on page 1 only. A row is held whole by `fo:keep-together="always"` on its own
  `style:table-row-properties` (that is what exports `w:cantSplit`), and a header
  row repeats on the next page only when it sits inside
  `table:table-header-rows` (that is `w:tblHeader`) — the same 150-row table with
  both measured 150 `w:cantSplit`, 1 `w:tblHeader`, and the header row on all 3
  pages. The row style must be in `office:automatic-styles`: the same style moved
  to `office:styles` exported 0 `w:cantSplit`. `style:row-height`, and
  `fo:keep-together` on the table properties, changed nothing.
- **pptx:** the page is 28 × 15.75 cm — LibreOffice's own 16:9 — and a
  `style:page-layout` naming any other 16:9 size is ignored, which misplaces every
  frame laid out for it. Text styles must be `style:family="paragraph"` with names
  no built-in Impress style uses (`Title` resolves to the built-in `Title` and the
  styling is lost). Paragraph spacing inside a slide's text box does not survive
  the export; use `fo:line-height`.
- **A numeric column is sized for a number, not for a number plus a currency.**
  The default quotation's amount column is 3.2 cm, which holds `1,200,000`; the
  same value with the currency suffix behind it runs past the right border and
  touches the notes column — measured with ` 円` and with ` JPY`, and identically
  on the template before the currency became data, so it is the column's width and
  not the placeholder. Keep the amounts shorter than seven digits, or widen the
  column in your copy while the five still add up to 16.5 cm.
- **Fix a copy of the template, never the filled file.** `fill` writes
  `<out>.fodt`/`.fodp`/`.fods` beside the output — the diffable record of what was
  filled — but `fill` re-reads its `--template` and rewrites that record on every
  run, so an edit there is discarded silently. The fix loop is three steps:
  `template docx --out work` makes an editable copy of the default template, you
  edit `work/report.fodt`, and `fill --template work/report.fodt …` re-substitutes
  and converts it.

## 4. The UNO leg, for a template someone handed over

```bash
bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" fill --template customer-form.docx --data data.json --out out/form.docx
```

It starts a headless LibreOffice with a pinned profile, opens the template
`Hidden`, replaces one key at a time so the report can name what matched nothing,
stores with the format's filter name, and terminates the office. Measured on a
docx holding a placeholder as a plain run, as two runs, inside a content control
and inside a mail-merge field: **five of six keys filled** — the run split in two
and the content control both worked, because the search runs over LibreOffice's
text model — and the one inside the `MERGEFIELD` result came back `unfilled`,
because a field's result is not part of the searchable text. Content controls and
fields survive the round trip, so the fix is to move that placeholder out of the
field (or let a person merge it), not to give up on the template.

The traps this had to be written around, all of them in the module comment:
`page.add` before `setString` for a shape added at run time; absolute paths
through `uno.systemPathToFileUrl`; `uno.Enum` rather than `getConstantByName`; a
killed office leaving `.~lock.<name>#` beside the file — which is why the fill
copies the template into a temporary directory rather than opening the caller's.

`bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" uno --script <file.py>` runs any UNO program against the
same listener, for the document a substitution cannot express.

## 5. The visual loop — render, read, fix, and say what you looked at

```bash
bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" render out/report.docx --outdir out/render --dpi 120   # every page
bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" render out/deck.pptx --outdir out/render --dpi 110 --pages 1-3
```

The render goes file → PDF → one PNG per page, and its JSON line carries each
page's path, pixel size and byte size. Read the images with the `read` tool, which
resolves a path under the run's checkout or under `/tmp/atomaton-workspace` — the
workspace is the only writable directory a run has that survives into the next
one, and `/tmp` outside it is not readable back. Render into the workspace or
beside the output, not into `/tmp`.

**Budget: pages × pixels, not tokens.** One A4 page at 120 dpi is 993 × 1404 px
and 40–190 KB; a 16:9 slide at 110 dpi is 1213 × 683. Three pages of report, three
slides and two sheets of a book are **eight images, roughly 1.5 MB, about 2 s to
render** — so the loop is cheap enough to run after *every* fix, and running it
three or four times is normal. Read every page; a defect that the first page
hides is what the second page is for.

Fix in the template, not in the output: the output is regenerated every time. The
editable thing is the template copy from §3 — `template docx --out work`, edit
`work/report.fodt`, then `fill --template work/report.fodt …` — not the filled flat
file beside the output, which the next `fill` overwrites.

What a render shows, and what only the file shows:

| A picture shows | Only the file shows |
| --- | --- |
| overflow past a cell or a frame edge, `###` where a number does not fit | a formula whose value is cached and never recalculated |
| a font that is not the one intended (serif where sans was asked for) | `w:lang w:eastAsia="en-US"` under a Japanese paragraph |
| a heading stranded at the foot of a page, a table split across two | a page layout that was dropped, so the print margins are the default |
| a table continuing onto the next page | a docx with no `w:cantSplit`, so a row may split mid-row, and no `w:tblHeader`, so no header repeats |
| an empty footer, a missing page number, a colour on the wrong background | a placeholder left as `{{KEY}}` inside a field result |

So the loop is two checks, not one: read the pages, then confirm the rules in
§3 for any attribute that cannot be seen.

**Who can run it.** The loop needs an agent that can read an image, and `vision`
is a property of the agent definition, not of the model underneath it: `engineer`
(`vision: true`) and `reviewer` (`vision: true`) run it as written. `atomaton` and
`architect` are `vision: false`, so the loop is not theirs to run and a fill
without a look is a half-finished job: they hand the document work to `engineer`,
which is where a document that has to be looked at belongs. Do not hand the images
to a delegate — every shipped delegate is `vision: false` too, so a delegate
receives no picture and answers about a page it never saw. If no vision-capable
agent is reachable, say in the report that the visual loop was not run and what is
therefore unverified; do not describe a page you have not seen.

## 6. What the report carries

One table, in this order, then the open items:

| Step | Command | Duration | Result |
| --- | --- | --- | --- |
| dependencies | `bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" deps` | 46 s first run, <1 s after | `installed` / `cached` per dependency |
| fill | `bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" fill …` | about 0.3 s flat, 0.5–1.5 s UNO | `keysNotInTemplate` / `leftInTemplate` / `unfilled` |
| render | `bun "${ATOMATON_MACHINERY_ROOT:-.}/.github/atomaton/skills/docs/customer-doc.mjs" render …` | 0.6–0.8 s for 3–4 pages | page count and pixel size per page |
| visual loop | (the agent reading the images) | | what was wrong, what was changed, what is still wrong |

Then: the paths of the produced files, the page count, and a **"what still needs
a person"** list — every `unfilled` key, every field that must be merged in Word,
every place the sample data had to be shortened to fit. The durations are from
the run that ran it, not from this page.

## 7. Where the files go

The produced docx/pptx/xlsx are **published, not committed** — `publish_draft`
onto the orphan branch: no `branch` argument, one call per publication,
`blob_url` per file in the report. Putting the file into a comment instead would
mean the endpoint `gh issue comment --attach` posts to, and an Actions token is
not on that endpoint's allowlist — measured, a body-bearing POST answers 404 — so
the orphan branch is the route there is.

What *is* committed is the template and the scripts: they are text, they diff,
and they are the thing the next run changes. Never commit a generated `.docx`,
`.pptx`, `.xlsx`, `.pdf` or page image.

## 8. The limitation, stated in the report

This procedure **detects and fixes** layout defects; it does not **guarantee**
the document is right. The loop sees a LibreOffice render on this machine: it
does not see pagination as Word will compute it, font substitution on the
customer's machine, a mail-merge field that Word will fill from a data source, or
a CJK line break that differs between engines. Say that in the report, name what
was checked and how, and let a person open the file before it goes out.
