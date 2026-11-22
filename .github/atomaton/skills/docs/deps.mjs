#!/usr/bin/env bun
/**
 * deps.mjs — the on-demand install that `docs/customer-doc` runs.
 *
 * `docs/customer-doc` writes all three Office formats, drives LibreOffice over
 * UNO and renders pages as images. Every leg of it needs the same LibreOffice
 * unpacked under a cache directory with `dpkg-deb -x` — no root, and nothing
 * added to `environment.setup_commands` — and the release tarball is a 213MB
 * download, so the install is here once and one cache serves every call after it.
 *
 * `ensure()` records every step in `deps` and on stderr, and a caller prints that
 * array as the `deps` of its JSON summary. A failed install is not a failed run:
 * the caller still produces what it can and says which dependency is missing.
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

/**
 * A directory that survives a run, so "first run installs, later runs skip" holds
 * across runs too. `XDG_CACHE_HOME` is what a live run sets to a directory it
 * owns (`atomaton-runner.yml`), and it is also the directory `HOME` is pointed at
 * during a conversion below, which is why the CJK fonts land inside it.
 */
export const CACHE = process.env.XDG_CACHE_HOME ?? join(process.env.HOME ?? tmpdir(), ".cache");
export const CACHE_DIR = join(CACHE, "atomaton-docgen");
mkdirSync(CACHE_DIR, { recursive: true });

/** Every dependency step this process took, in order. */
export const deps = [];

export function record(name, state, seconds) {
  deps.push({ name, state, seconds: Math.round(seconds * 10) / 10 });
  console.error(`deps: ${name}: ${state} (${Math.round(seconds * 10) / 10}s)`);
}

const secondsSince = (t) => (Date.now() - t) / 1000;

// ------------------------------------------------------------------ LibreOffice

const LO_RELEASE = "25.8.7";
const LO_LINK = `https://download.documentfoundation.org/libreoffice/stable/${LO_RELEASE}/deb/x86_64/LibreOffice_${LO_RELEASE}_Linux_x86-64_deb.tar.gz`;
const LO_ROOT = join(CACHE_DIR, "lo-root");
const LO_PROGRAM = join(LO_ROOT, "opt", "libreoffice25.8", "program");

/**
 * The `.debs` to unpack, and the reason one list serves both skills.
 *
 * The release tarball arrives whole — the download is the expensive part and it
 * is the same one whoever asked — so a narrower list saves only extraction.
 * Measured on the runner, from a cached tarball: these fifteen are 583MB and 4.3s
 * (0.6s of `tar`, 3.7s of `dpkg-deb -x`); the nine a PDF conversion alone needs
 * are 507MB and 3.8s. Two lists would buy that 0.5s and 76MB and cost a second
 * cache directory whose completeness every caller would then have to judge for
 * itself, which is how a cache comes to say `cached` about something it does not
 * have.
 */
const DEBS = [
  "libreoffice25.8_*.deb", // the program
  "libreoffice25.8-ure_*.deb", // libuno_sal
  "libreoffice25.8-en-us_*.deb", // UI language
  "libobasis25.8-en-us_*.deb", // the en-US langpack; without it startup dies on "The user interface language cannot be determined."
  "libobasis25.8-core_*.deb",
  "libreoffice25.8-writer_*.deb",
  "libobasis25.8-writer_*.deb",
  "libreoffice25.8-calc_*.deb", // libsclo.so
  "libobasis25.8-calc_*.deb",
  "libreoffice25.8-impress_*.deb",
  "libobasis25.8-impress_*.deb", // libsdlo.so
  "libobasis25.8-pyuno_*.deb", // program/python, pyuno.so and uno.py: the UNO bridge
  "libobasis25.8-graphicfilter_*.deb",
  "libobasis25.8-ooofonts_*.deb",
  "libobasis25.8-images_*.deb",
];

/**
 * The interpreter that can `import uno` for a given office, or undefined.
 *
 * UNO's Python ships with LibreOffice — the pyuno deb brings `program/python`,
 * `uno.py` and `pyuno.so` beside the binary — so the interpreter is the one
 * *beside the office the caller is about to use*: the cached tarball's, or the
 * sibling of the `soffice` the PATH resolves to, which a distro package keeps in
 * the same layout. Never a mix of the two: a bridge driven from another build's
 * interpreter fails in the bridge, naming nothing that points here.
 *
 * The candidate is probed rather than assumed. A distro can ship the office
 * without an interpreter that sees `uno`, and then unpacking the tarball is what
 * has to happen.
 */
export function unoPython(sofficePath) {
  let program;
  try {
    program = dirname(sofficePath === "soffice" ? realpathSync(onPath("soffice") ?? "soffice") : sofficePath);
  } catch {
    // A PATH `soffice` that vanished or resolves through a broken link: there is no
    // interpreter to name, and the caller installs the tarball instead of throwing.
    return undefined;
  }
  const interpreter = join(program, "python");
  if (!existsSync(interpreter)) return undefined;
  return spawnSync(interpreter, ["-c", "import uno"], { encoding: "utf8" }).status === 0 ? interpreter : undefined;
}

/** Where `name` resolves to on the PATH, without a shell and without `which`. */
function onPath(name) {
  for (const dir of (process.env.PATH ?? "").split(":")) {
    if (!dir) continue;
    const candidate = join(dir, name);
    if (existsSync(candidate)) return candidate;
  }
  return undefined;
}

/**
 * LibreOffice, unpacked under the cache; the `soffice` path, or undefined.
 *
 * `program/python` is half of the completeness marker rather than `soffice`
 * alone: UNO runs on the interpreter the pyuno deb brings, and a cache left by a
 * run that only converted documents has `soffice` without it. Checking one
 * binary would call that cache `cached` and fail later, in the bridge, naming
 * nothing that points here.
 *
 * With `uno` the office and its interpreter are chosen together, and a PATH
 * office counts only when the `python` beside it can `import uno`. Answering
 * `"soffice"` on the strength of `--version` alone is how `deps` came to report
 * LibreOffice present on a machine where the UNO leg could never run — and where
 * the fix it printed ("run `deps`") took the same early return and installed
 * nothing.
 */
export function libreOffice({ uno = false } = {}) {
  const onPathOffice = spawnSync("soffice", ["--version"], { encoding: "utf8" });
  const onPathUsable = onPathOffice.status === 0;
  // A conversion needs only the binary. The UNO leg needs the pair, so there the
  // unpacked tarball is preferred — it is one build end to end, and it is already
  // on disk for every run after the first.
  if (onPathUsable && !uno) return "soffice";
  const t0 = Date.now();
  const sofficePath = join(LO_PROGRAM, "soffice");
  // `program/python` present is the completeness marker, and for UNO it is not the
  // whole question: an interpreter left by a half-finished extraction exists and
  // cannot `import uno`. The probe runs only when UNO is asked for, because a
  // conversion should not pay for it.
  if (existsSync(sofficePath) && existsSync(join(LO_PROGRAM, "python")) && (!uno || unoPython(sofficePath))) {
    record("libreoffice", "cached", secondsSince(t0));
    return sofficePath;
  }

  if (uno && onPathUsable && unoPython("soffice")) return "soffice";
  const tgz = join(CACHE_DIR, "lo.tar.gz");
  if (!existsSync(tgz)) {
    const dl = spawnSync("curl", ["-sSL", "-o", tgz, LO_LINK], { encoding: "utf8" });
    if (dl.status !== 0 || !existsSync(tgz)) {
      record("libreoffice", "failed", secondsSince(t0));
      return undefined;
    }
  }
  const extractDir = mkdtempSync(join(tmpdir(), "lo-debs-"));
  try {
    // One `tar` call for every pattern: a call per pattern walks the 213MB archive
    // once each -- 8.7s for these fifteen, against 0.6s for a single pass.
    spawnSync("tar", ["-xzf", tgz, "-C", extractDir, "--wildcards", ...DEBS.map((p) => `LibreOffice_*/DEBS/${p}`)]);
    const debDir = readdirSync(extractDir).map((d) => join(extractDir, d, "DEBS")).find((d) => existsSync(d));
    if (!debDir) {
      record("libreoffice", "failed", secondsSince(t0));
      return undefined;
    }
    for (const deb of readdirSync(debDir)) {
      spawnSync("dpkg-deb", ["-x", join(debDir, deb), LO_ROOT]);
    }
  } finally {
    rmSync(extractDir, { recursive: true, force: true });
  }
  const ok = existsSync(sofficePath) && existsSync(join(LO_PROGRAM, "python"));
  record("libreoffice", ok ? "installed" : "failed", secondsSince(t0));
  return ok ? sofficePath : undefined;
}

/**
 * CJK fonts, best effort: without them a Japanese document renders as tofu,
 * which is a quality problem, not a delivery problem — and one the visual loop
 * cannot tell from a font-size defect, so it is the loop's own prerequisite.
 *
 * They go into `$CACHE_DIR/.fonts`, which is `~/.fonts` for every LibreOffice
 * invocation below (`HOME` is `$CACHE_DIR`). It also reads
 * `$XDG_DATA_HOME/fonts`, and that is the path this used to install into; a live
 * run sets `XDG_DATA_HOME` to the runner's own cache, where nothing here writes,
 * so the fonts were installed, reported `installed`, and never read.
 */
export function cjkFonts() {
  const fontsDir = join(CACHE_DIR, ".fonts");
  if (existsSync(fontsDir)) {
    record("fonts-noto-cjk", "cached", 0);
    return;
  }
  if (existsSync("/usr/share/fonts/opentype/noto")) {
    record("fonts-noto-cjk", "system", 0);
    return;
  }
  const t0 = Date.now();
  const dl = spawnSync("apt-get", ["download", "fonts-noto-cjk"], { cwd: CACHE_DIR, encoding: "utf8" });
  const deb = readdirSync(CACHE_DIR).find((f) => f.startsWith("fonts-noto-cjk") && f.endsWith(".deb"));
  if (dl.status !== 0 || !deb) {
    record("fonts-noto-cjk", "failed", secondsSince(t0));
    return;
  }
  const root = join(CACHE_DIR, "font-root");
  spawnSync("dpkg-deb", ["-x", join(CACHE_DIR, deb), root]);
  mkdirSync(fontsDir, { recursive: true });
  for (const f of readdirSync(join(root, "usr/share/fonts/opentype/noto"))) {
    copyFileSync(join(root, "usr/share/fonts/opentype/noto", f), join(fontsDir, f));
  }
  rmSync(root, { recursive: true, force: true });
  record("fonts-noto-cjk", "installed", secondsSince(t0));
}

// ------------------------------------------------------------------ Python

/**
 * A virtualenv holding `packages`, as `[{ pip, module }]`; the interpreter, or
 * undefined when no `python3` has `venv`.
 *
 * The venv is shared by both callers and each probes only its own packages, so a
 * machine that has already run one skill installs nothing for the packages they
 * share — there are none today, and `cached` is decided per package rather than
 * per venv for exactly that reason.
 */
export function python(packages) {
  const t0 = Date.now();
  let base;
  for (const bin of ["python3", "python"]) {
    const probe = spawnSync(bin, ["-c", "import venv"], { encoding: "utf8" });
    if (probe.status === 0) {
      base = bin;
      break;
    }
  }
  if (!base) {
    record("python3-venv", "failed", secondsSince(t0));
    return undefined;
  }
  const venvDir = join(CACHE_DIR, "venv");
  const interpreter = join(venvDir, "bin", "python");
  if (existsSync(interpreter)) {
    record("python-venv", "cached", secondsSince(t0));
  } else {
    const made = spawnSync(base, ["-m", "venv", venvDir], { encoding: "utf8" });
    if (made.status !== 0) {
      record("python-venv", "failed", secondsSince(t0));
      return undefined;
    }
    record("python-venv", "installed", secondsSince(t0));
  }
  // The distribution name pip installs is not the name the module imports under,
  // and for these it never agrees: `python-docx` imports as `docx` and
  // `python-pptx` as `pptx`. Deriving the import from the pip name by replacing
  // `-` with `_` therefore probes `python_docx`, which is not importable even when
  // the package is installed -- so the probe failed every run and the package was
  // reinstalled every run. Measured: the second run reported `python-docx:
  // installed (0.2s)` where the whole point of the install step is that it says
  // `cached`.
  for (const { pip, module } of packages) {
    const t1 = Date.now();
    const probe = spawnSync(interpreter, ["-c", `import ${module}`], { encoding: "utf8" });
    if (probe.status === 0) {
      record(pip, "cached", secondsSince(t1));
      continue;
    }
    const installed = spawnSync(join(venvDir, "bin", "pip"), ["install", "--quiet", pip], { encoding: "utf8" });
    record(pip, installed.status === 0 ? "installed" : "failed", secondsSince(t1));
  }
  return interpreter;
}

/**
 * Everything a caller needs, in the order the failures matter: LibreOffice, then
 * the fonts it renders with, then the interpreter.
 */
export function ensure({ lo = false, cjk = false, packages = [], uno = false } = {}) {
  const soffice = lo || uno ? libreOffice({ uno }) : undefined;
  if (cjk) cjkFonts();
  const interpreter = packages.length > 0 ? python(packages) : undefined;
  return { soffice, python: interpreter, unoPython: uno && soffice ? unoPython(soffice) : undefined };
}

// ------------------------------------------------------------------ conversion

/**
 * `soffice --convert-to`, with the profile pinned under the cache and `HOME`
 * pointed at the fonts.
 *
 * `--convert-to` overwrites an existing output, which is what makes re-rendering
 * into the same directory after a fix the same command as the first render.
 * Nothing else is redirected: the caller's `XDG_DATA_HOME` and `XDG_CACHE_HOME`
 * are inherited, because a run sets them to directories it owns and LibreOffice's
 * own profile is pinned by `-env:UserInstallation` here.
 *
 * `target` is the format name: `docx`, `pptx`, `xlsx`, `pdf`, `fodt`, ...
 * Returns `{ ok, seconds, path }` with `path` what the conversion should have
 * written.
 */
export function convert(sofficePath, { file, to, outdir }) {
  const t0 = Date.now();
  const r = spawnSync(
    sofficePath,
    [
      "--headless",
      "--norestore",
      `-env:UserInstallation=file://${join(CACHE_DIR, "lo-home")}`,
      "--convert-to",
      to,
      "--outdir",
      outdir,
      file,
    ],
    { encoding: "utf8", env: { ...process.env, HOME: CACHE_DIR } },
  );
  const base = file.slice(file.lastIndexOf("/") + 1).replace(/\.[^.]+$/, "");
  const path = join(outdir, `${base}.${to}`);
  return { ok: r.status === 0 && existsSync(path), seconds: secondsSince(t0), path, stderr: r.stderr };
}
