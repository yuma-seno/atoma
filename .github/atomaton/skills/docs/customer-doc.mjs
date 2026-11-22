#!/usr/bin/env bun
/**
 * customer-doc.mjs — fill a template and render the result, for the three Office
 * formats a customer opens: docx, pptx, xlsx.
 *
 *     bun customer-doc.mjs deps
 *     bun customer-doc.mjs template docx [--out DIR]
 *     bun customer-doc.mjs fill --template <path|docx|pptx|xlsx> --data <json> --out <path>
 *     bun customer-doc.mjs render <file> --outdir DIR [--dpi 150] [--pages 1-3]
 *     bun customer-doc.mjs uno --script <file.py> [--args ...]
 *
 * Two legs, and which one runs is decided by the template:
 *
 * - a `.fodt`/`.fodp`/`.fods` template (the shipped ones under `templates/`, or
 *   the agent's copy of one) is filled **as text** — placeholders substituted in
 *   the XML, then `--convert-to` the requested binary format. The filled flat
 *   file is kept next to the output as the diffable record of what was filled —
 *   a fix is an edit to a copy of the template (`template <fmt> --out DIR`) and
 *   one more fill, not a round trip through a binary.
 * - any other template (a customer's `.docx`, `.pptx`, `.xlsx` — the ones with a
 *   theme, content controls and mail-merge fields) is filled **over UNO**: a
 *   headless LibreOffice is started, the template opened, the placeholders
 *   replaced, and the result stored in the same format.
 *
 * Every subcommand ends with one JSON line on stdout and progress on stderr, so a
 * run reads the result the same way every time.
 */
import { spawn, spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { CACHE_DIR, convert, deps, ensure, record } from "./deps.mjs";

const HERE = dirname(new URL(import.meta.url).pathname);
const TEMPLATES = join(HERE, "templates");

/** The shipped default template per output format, as flat ODF text. */
const DEFAULT_TEMPLATES = {
  docx: "report.fodt",
  pptx: "deck.fodp",
  xlsx: "book.fods",
};

/** The filter LibreOffice needs to write each format; the extension alone is not enough for UNO. */
const FILTERS = {
  docx: "MS Word 2007 XML",
  pptx: "Impress MS PowerPoint 2007 XML",
  xlsx: "Calc MS Excel 2007 XML",
  pdf: "writer_pdf_Export",
  odt: "writer8",
  otf: "writer8",
  odp: "impress8",
  ods: "calc8",
};

const argv = process.argv.slice(2);
const command = argv[0];
const rest = argv.slice(1);

function flag(name) {
  const i = rest.indexOf(name);
  return i >= 0 ? rest[i + 1] : undefined;
}

function fail(message) {
  console.error(`customer-doc: ${message}`);
  process.exit(1);
}

/** The one line a caller reads: one JSON object, no indent, so the last line of stdout is the summary. */
function emit(result) {
  console.log(JSON.stringify({ ...result, deps }));
}

const extensionOf = (path) => (/\.[^.\\/]+$/.exec(path)?.[0] ?? "").slice(1).toLowerCase();

// ------------------------------------------------------------------- deps

/** Install and report everything the other subcommands use, so a run can time it once. */
function commandDeps() {
  const { soffice, python, unoPython } = ensure({
    lo: true,
    cjk: true,
    packages: [
      { pip: "pypdfium2", module: "pypdfium2" },
      { pip: "pillow", module: "PIL" },
    ],
    uno: true,
  });
  emit({ soffice, python, unoPython });
}

// ------------------------------------------------------------------- template

/**
 * The default template for a format, as a path; with `--out`, a copy of it.
 *
 * The copy is the point of the leg the skill calls "fix it in the text": the
 * shipped file is read-only in spirit (it is the template every run starts from),
 * and an edit belongs in the run's own copy, converted again to see the result.
 */
function commandTemplate() {
  const format = rest[0];
  const name = DEFAULT_TEMPLATES[format];
  if (!name) fail(`usage: customer-doc.mjs template <${Object.keys(DEFAULT_TEMPLATES).join("|")}> [--out DIR]`);
  const source = join(TEMPLATES, name);
  if (!existsSync(source)) fail(`default template missing: ${source}`);
  const outdir = flag("--out");
  if (!outdir) {
    emit({ format, template: source });
    return;
  }
  mkdirSync(outdir, { recursive: true });
  const target = join(outdir, name);
  copyFileSync(source, target);
  emit({ format, template: target });
}

// ------------------------------------------------------------------- fill

/** The template path `fill` was given: a shipped format name, or a file. */
function templatePath(value) {
  if (!value) fail("fill: --template <path|docx|pptx|xlsx> is required");
  const name = DEFAULT_TEMPLATES[value];
  if (!name) return resolve(value);
  const shipped = join(TEMPLATES, name);
  if (!existsSync(shipped)) fail(`default template missing: ${shipped}`);
  return shipped;
}

function readData(path) {
  if (!path) fail("fill: --data <json> is required");
  let raw;
  try {
    raw = readFileSync(resolve(path), "utf8");
  } catch {
    fail(`fill: cannot read --data ${path}`);
  }
  let data;
  try {
    data = JSON.parse(raw);
  } catch (e) {
    fail(`fill: --data ${path} is not JSON: ${e.message}`);
  }
  if (typeof data !== "object" || data === null || Array.isArray(data)) fail("fill: --data must be a JSON object");
  // `{{KEY}}` is the placeholder a flat ODF template can hold without any
  // machinery, and the key is what goes inside it; a nested value has no spelling
  // there, so it is refused here rather than silently becoming "[object Object]".
  for (const [key, value] of Object.entries(data)) {
    if (typeof value === "object" && value !== null) fail(`fill: --data.${key} must be a string or a number`);
  }
  return data;
}

const xmlEscape = (text) => String(text).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");

const PLACEHOLDER = /\{\{[A-Z0-9_]+\}\}/g;

/**
 * The flat ODF leg: substitute in the XML, then convert.
 *
 * The filled flat file stays in the output directory as the diffable record of
 * what was filled. It is not the thing a fix edits: this function re-reads its
 * `--template` and rewrites the record on every run, so an edit there is
 * discarded with no error. The fix loop the skill documents edits a copy of the
 * template (`template <fmt> --out DIR`) and passes that copy as `--template`.
 */
function fillFlat({ template, data, out }) {
  const t0 = Date.now();
  // The flat file lands beside the output, and the caller's `--out` names a
  // directory that need not exist yet: the skill's own first example is
  // `--out out/report.docx` on a tree with no `out/`, and the UNO leg already
  // creates it (`fillUno`). Without this the write below throws ENOENT before any
  // conversion has happened, which reads as a broken script rather than a missing
  // directory.
  mkdirSync(dirname(resolve(out)), { recursive: true });
  let text = readFileSync(template, "utf8");
  const missing = [];
  for (const [key, value] of Object.entries(data)) {
    const placeholder = `{{${key}}}`;
    if (!text.includes(placeholder)) missing.push(key);
    // A function replacement, not a string one: in a string replacement, a
    // dollar sign followed by an ampersand or a quote in the replacement text
    // is a pattern — it can re-insert the placeholder or the rest of the
    // document — so a currency value written as USD with a thousands
    // apostrophe after the dollar sign would duplicate the document tail. The
    // function form inserts the escaped value literally.
    text = text.replaceAll(placeholder, () => xmlEscape(value));
  }
  const target = extensionOf(out);
  // Asked for the template's own format, the filled file *is* the output: naming it
  // a second way from `out` gave two names for one file, and the copy that followed
  // was a file onto itself — which leaves the bytes alone here and is not something
  // to rest on. One name, written once.
  const sameFormat = target === extensionOf(template);
  const flat = sameFormat
    ? resolve(out)
    : join(dirname(resolve(out)), `${basename(out, `.${target}`)}.${extensionOf(template)}`);
  writeFileSync(flat, text);
  const leftover = [...new Set(text.match(PLACEHOLDER) ?? [])];

  if (sameFormat) {
    return {
      leg: "flat-odf",
      template,
      out: resolve(out),
      flatOdf: flat,
      converted: false,
      keysNotInTemplate: missing,
      leftInTemplate: leftover,
      unfilled: leftover,
      seconds: (Date.now() - t0) / 1000,
    };
  }
  const soffice = ensure({ lo: true }).soffice;
  if (!soffice) fail("fill: LibreOffice is not available, so the flat ODF file could not be converted");
  const converted = convert(soffice, { file: flat, to: target, outdir: dirname(resolve(out)) });
  if (!converted.ok) fail(`fill: conversion to ${target} failed: ${(converted.stderr ?? "").slice(0, 400)}`);
  if (converted.path !== resolve(out)) copyFileSync(converted.path, resolve(out));
  return {
    leg: "flat-odf",
    template,
    out: resolve(out),
    flatOdf: flat,
    converted: true,
    keysNotInTemplate: missing,
    leftInTemplate: leftover,
    unfilled: leftover,
    seconds: (Date.now() - t0) / 1000,
  };
}

/**
 * The UNO fill program for a binary template.
 *
 * It replaces one key at a time so the report can name the keys that matched
 * nothing — a template whose placeholder sits in a content control or a field, or
 * whose placeholder Word split across two runs, comes back as `unfilled` instead
 * of as a document that quietly still says `{{CLIENT}}`.
 *
 * The traps are the ones this had to be written around:
 *
 * - a shape's text is set with `setString`, and for a shape added at run time that
 *   only works *after* `page.add(s)` — the reverse order stores an empty shape;
 * - a path must be absolute and go through `uno.systemPathToFileUrl`, because a
 *   relative path is resolved against the office process, not this one;
 * - an enumeration value is `uno.Enum(...)`; `getConstantByName` raises;
 * - the listener is shut down with `desktop.terminate()` before the process
 *   exits, because killing it leaves `.~lock.<name>#` beside the file it had open
 *   and the next store into that directory fails with `Io/Abort Code:27`.
 */
const UNO_FILL = String.raw`
import json, os, sys, time, uno
from com.sun.star.beans import PropertyValue

def prop(name, value):
    p = PropertyValue(); p.Name = name; p.Value = value; return p

template, out, filt, data_path = sys.argv[1:5]
data = json.load(open(data_path, encoding="utf-8"))
t0 = time.time()
local = uno.getComponentContext()
resolver = local.ServiceManager.createInstanceWithContext("com.sun.star.bridge.UnoUrlResolver", local)
ctx = None
for _ in range(120):
    try:
        ctx = resolver.resolve("uno:socket,host=localhost,port=%s;urp;StarOffice.ComponentContext" % os.environ["CUSTOMER_DOC_PORT"])
        break
    except Exception:
        time.sleep(0.25)
if ctx is None:
    sys.exit("customer-doc: could not reach the LibreOffice listener")
desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
doc = desktop.loadComponentFromURL(uno.systemPathToFileUrl(os.path.abspath(template)), "_blank", 0, (prop("Hidden", True),))
hits = {}

def text_shapes(container):
    """Every text-bearing shape, descending into groups -- a customer deck groups things."""
    for i in range(container.getCount()):
        shape = container.getByIndex(i)
        if shape.supportsService("com.sun.star.drawing.GroupShape"):
            for inner in text_shapes(shape):
                yield inner
        elif hasattr(shape, "getString"):
            yield shape

if doc.supportsService("com.sun.star.sheet.SpreadsheetDocument"):
    sheets = doc.getSheets()
    for i in range(sheets.getCount()):
        sheet = sheets.getByIndex(i)
        for key, value in data.items():
            d = sheet.createReplaceDescriptor()
            d.SearchString = "{{%s}}" % key
            d.ReplaceString = str(value)
            hits[key] = hits.get(key, 0) + sheet.replaceAll(d)
elif doc.supportsService("com.sun.star.presentation.PresentationDocument"):
    pages = doc.getDrawPages()
    for i in range(pages.getCount()):
        for shape in text_shapes(pages.getByIndex(i)):
            text = shape.getString()
            if "{{" not in text:
                continue
            replaced = text
            for key, value in data.items():
                if "{{%s}}" % key in replaced:
                    replaced = replaced.replace("{{%s}}" % key, str(value))
                    hits[key] = hits.get(key, 0) + 1
            if replaced != text:
                shape.setString(replaced)
else:
    for key, value in data.items():
        d = doc.createSearchDescriptor()
        d.SearchString = "{{%s}}" % key
        d.ReplaceString = str(value)
        hits[key] = doc.replaceAll(d)

doc.storeToURL(uno.systemPathToFileUrl(os.path.abspath(out)), (prop("FilterName", filt), prop("Overwrite", True)))
doc.close(False)
desktop.terminate()
print(json.dumps({"hits": hits, "fill_seconds": round(time.time() - t0, 2)}))
`;

const PORT = process.env.CUSTOMER_DOC_PORT ?? "2002";

/**
 * A live LibreOffice listener, the caller's UNO program run against it, and the
 * office shut down again.
 *
 * `soffice --accept` runs in the foreground for as long as the office lives, so
 * it is a child process and this waits for it to exit after the program is done.
 * The profile is pinned (`-env:UserInstallation`) and reused by the next call, so
 * an office still holding it would make the next listener die with "another
 * instance is running" before any of this code runs.
 *
 * A program that ends with `desktop.terminate()` takes the office down itself;
 * when it does not, the child is killed here -- and a killed office leaves
 * `.~lock.<name>#` beside every document it had open, which is why the fill opens
 * a copy in a directory it then deletes rather than the caller's file.
 */
async function withListener(scriptPath, scriptArgs) {
  // `uno: true` asks for an office *and* an interpreter that can import `uno`.
  // Asking only for the office accepted a PATH binary the bridge could never be
  // driven from, and then failed here naming `deps` as the fix.
  const { soffice, unoPython: interpreter } = ensure({ uno: true });
  if (!soffice) fail("LibreOffice is not available, so there is nothing to drive over UNO");
  if (!interpreter) fail("no LibreOffice interpreter can `import uno`; run `customer-doc.mjs deps`");
  const child = spawn(
    soffice,
    [
      "--headless",
      "--norestore",
      "--invisible",
      `-env:UserInstallation=file://${join(CACHE_DIR, "lo-uno")}`,
      `--accept=socket,host=localhost,port=${PORT};urp;`,
    ],
    { stdio: ["ignore", "ignore", "pipe"], env: { ...process.env, HOME: CACHE_DIR } },
  );
  const run = spawnSync(interpreter, [scriptPath, ...scriptArgs], {
    encoding: "utf8",
    timeout: 300_000,
    env: { ...process.env, CUSTOMER_DOC_PORT: PORT, HOME: CACHE_DIR },
  });
  child.kill();
  await new Promise((done) => {
    if (child.exitCode !== null) return done();
    child.once("exit", done);
    setTimeout(done, 5_000);
  });
  return run;
}

async function fillUno({ template, data, out }) {
  const t0 = Date.now();
  const target = extensionOf(out);
  const filter = FILTERS[target];
  if (!filter) fail(`fill: no filter for .${target}; one of ${Object.keys(FILTERS).join(", ")}`);
  // The template is copied out of wherever it lives before LibreOffice opens it:
  // opening it in place drops a `.~lock.<name>#` beside it, which in the shipped
  // template's case is the repository's own working tree.
  const work = mkdtempSync(join(tmpdir(), "customer-doc-fill-"));
  const source = join(work, `source.${extensionOf(template) || target}`);
  copyFileSync(template, source);
  const dataPath = join(work, "data.json");
  writeFileSync(dataPath, JSON.stringify(data));
  const program = join(work, "fill.py");
  writeFileSync(program, UNO_FILL);
  mkdirSync(dirname(resolve(out)), { recursive: true });
  try {
    const run = await withListener(program, [source, resolve(out), filter, dataPath]);
    return run;
  } finally {
    rmSync(work, { recursive: true, force: true });
    record("uno-fill", "done", (Date.now() - t0) / 1000);
  }
}

async function commandFill() {
  const template = templatePath(flag("--template"));
  const data = readData(flag("--data"));
  const out = flag("--out");
  if (!out) fail("fill: --out <path> is required");
  if (!existsSync(template)) fail(`fill: template not found: ${template}`);
  if (!statSync(template).isFile()) fail(`fill: ${template} is not a file`);

  if (/\.fod[tps]$/i.test(template)) {
    emit(fillFlat({ template, data, out }));
    return;
  }
  const run = await fillUno({ template, data, out });
  if (run.status !== 0) fail(`fill: UNO fill failed: ${(run.stderr || run.stdout || "").slice(0, 800)}`);
  const parsed = JSON.parse((run.stdout ?? "").trim().split("\n").pop() || "{}");
  emit({
    leg: "uno",
    template,
    out: resolve(out),
    unfilled: Object.keys(data).filter((key) => (parsed.hits?.[key] ?? 0) === 0),
    hits: parsed.hits ?? {},
    fillSeconds: parsed.fill_seconds,
  });
}

// ------------------------------------------------------------------- render

/**
 * Pages to PNG, through a PDF.
 *
 * `--convert-to png` makes exactly one image per document — the first page — so a
 * deck or a multi-page report would be judged from page 1 alone. The PDF is
 * rendered page by page instead, and it is itself worth keeping: it is the format
 * a customer reads.
 */
const RASTERIZE = String.raw`
import json, sys
import pypdfium2 as pdfium

pdf, outdir, dpi, pages = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
doc = pdfium.PdfDocument(pdf)
count = len(doc)
if pages in ("", "all"):
    selected = list(range(1, count + 1))
else:
    selected = []
    for part in pages.split(","):
        if "-" in part:
            first, last = part.split("-", 1)
            selected.extend(range(int(first), int(last) + 1))
        else:
            selected.append(int(part))
out = []
for number in selected:
    page = doc[number - 1]
    image = page.render(scale=dpi / 72).to_pil()
    path = "%s/page-%d.png" % (outdir, number)
    image.save(path)
    out.append({"page": number, "path": path, "width": image.size[0], "height": image.size[1]})
print(json.dumps({"pages": count, "images": out}))
`;

async function commandRender() {
  const file = rest.find((a) => !a.startsWith("--") && a !== flag("--outdir") && a !== flag("--dpi") && a !== flag("--pages"));
  if (!file) fail("render: usage: render <file> --outdir DIR [--dpi 150] [--pages 1-3]");
  const outdir = resolve(flag("--outdir") ?? "customer-doc-render");
  const dpi = Number(flag("--dpi") ?? 150);
  mkdirSync(outdir, { recursive: true });
  const t0 = Date.now();
  const { soffice, python } = ensure({
    lo: true,
    cjk: true,
    packages: [
      { pip: "pypdfium2", module: "pypdfium2" },
      { pip: "pillow", module: "PIL" },
    ],
  });
  if (!soffice) fail("render: LibreOffice is not available");
  if (!python) fail("render: no python with a venv, so the PDF cannot be rasterized");

  const converted = convert(soffice, { file: resolve(file), to: "pdf", outdir });
  if (!converted.ok) fail(`render: conversion to PDF failed: ${(converted.stderr ?? "").slice(0, 400)}`);
  record("pdf-convert", "done", converted.seconds);

  const program = join(outdir, ".rasterize.py");
  writeFileSync(program, RASTERIZE);
  const run = spawnSync(python, [program, converted.path, outdir, String(dpi), flag("--pages") ?? "all"], {
    encoding: "utf8",
    timeout: 300_000,
  });
  rmSync(program, { force: true });
  if (run.status !== 0) fail(`render: rasterizing failed: ${(run.stderr || "").slice(0, 400)}`);
  const parsed = JSON.parse(run.stdout.trim().split("\n").pop() || "{}");
  emit({
    file: resolve(file),
    pdf: converted.path,
    dpi,
    pages: parsed.pages,
    images: (parsed.images ?? []).map((image) => ({ ...image, bytes: statSync(image.path).size })),
    seconds: (Date.now() - t0) / 1000,
  });
}

// ------------------------------------------------------------------- uno

/**
 * Run the caller's own UNO program against a live listener.
 *
 * Everything in this file that touches a document does it through `runUno`, and a
 * structure the fill does not cover — a slide added, a table row inserted, a
 * series styled — is the same kind of edit. This exposes the listener and the
 * bundled interpreter so that such an edit is a script of its own rather than a
 * second copy of the start-up sequence above.
 */
async function commandUno() {
  const script = flag("--script");
  if (!script) fail("uno: usage: uno --script <file.py> [--args ...]");
  const i = rest.indexOf("--args");
  const args = i >= 0 ? rest.slice(i + 1) : [];
  if (!existsSync(resolve(script))) fail(`uno: ${script} not found`);
  const run = await withListener(resolve(script), args);
  const output = (run.stdout ?? "").trim();
  let parsed;
  try {
    parsed = JSON.parse(output.split("\n").pop());
  } catch {
    parsed = { output };
  }
  emit({ script: resolve(script), exit: run.status, ...parsed, stderr: run.status === 0 ? undefined : (run.stderr ?? "").slice(0, 800) });
  if (run.status !== 0) process.exitCode = 1;
}

// ------------------------------------------------------------------- main

const commands = { deps: commandDeps, template: commandTemplate, fill: commandFill, render: commandRender, uno: commandUno };
const chosen = commands[command];
if (!chosen) {
  console.error(`customer-doc: usage: customer-doc.mjs <${Object.keys(commands).join("|")}> ...`);
  process.exit(1);
}
await chosen();
