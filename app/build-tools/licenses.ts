import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { REPO_ROOT } from './versions.ts'

export interface LicenseText { file: string; text: string }
export interface Component { source: 'cargo' | 'npm'; name: string; version: string; license: string | null; texts: LicenseText[] }
export interface CargoPackage { id: string; name: string; version: string; license: string | null; manifest_path: string; source: string | null }
export interface CargoMetadata { packages: CargoPackage[]; resolve: { root: string | null; nodes: { id: string; deps: { pkg: string; dep_kinds: { kind: string | null }[] }[] }[] } }
export interface NpmPackage { name: string; version: string; license: string | null; dir: string }

/** SPDX ids Aurora may ship. Anything else needs a human decision (add it here with a reason). */
export const ALLOWED = new Set([
  'MIT', 'MIT-0', 'Apache-2.0', 'Apache-2.0 WITH LLVM-exception', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', '0BSD',
  'MPL-2.0', 'Unicode-3.0', 'Unicode-DFS-2016', 'CC0-1.0', 'BSL-1.0', 'Unlicense', 'OFL-1.1', 'BlueOak-1.0.0', 'CDLA-Permissive-2.0',
])

/**
 * Crates whose Cargo.toml lacks a usable `license` field, with the license their repository states.
 * Each entry needs a comment naming where the license was read.
 */
export const LICENSE_OVERRIDES: Record<string, string> = {}

/**
 * Vendored C libraries whose license file sits below the crate root (paths relative to it). The
 * executor fills these from the crates in ~/.cargo/registry/src when the first run reports them missing.
 */
export const EXTRA_TEXTS: Record<string, string[]> = {
  // libwebp-sys 0.9.6: the vendored libwebp's BSD-3-Clause text is vendor/COPYING (the crate root has no license file).
  'libwebp-sys': ['vendor/COPYING'],
  // fdk-aac-sys 0.5.0: the Fraunhofer FDK AAC license is aac/NOTICE (the crate root has no license file; its Cargo.toml says MIT for the wrapper only).
  'fdk-aac-sys': ['aac/NOTICE'],
}

/** Crates that must contribute at least one license text (vendored C code we redistribute). */
export const REQUIRED_TEXTS = ['opusic-sys', 'libsqlite3-sys', 'libwebp-sys']
export const REQUIRED_TEXTS_FDK = ['fdk-aac-sys']

const TEXT_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense)([.-].*)?$/i

export function licenseOk(expr: string | null): boolean {
  if (!expr) return false
  const e = expr.trim().replace(/\s*\/\s*/g, ' OR ')
  // Split on top-level OR, then AND; parentheses are evaluated recursively.
  const strip = (s: string) => { s = s.trim(); while (s.startsWith('(') && s.endsWith(')') && balanced(s.slice(1, -1))) s = s.slice(1, -1).trim(); return s }
  const balanced = (s: string) => { let d = 0; for (const c of s) { if (c === '(') d++; if (c === ')' && --d < 0) return false } return d === 0 }
  const split = (s: string, op: string) => { const parts: string[] = []; let d = 0, start = 0; const re = new RegExp(`\\s${op}\\s`, 'y'); for (let i = 0; i < s.length; i++) { if (s[i] === '(') d++; else if (s[i] === ')') d--; else if (d === 0) { re.lastIndex = i; const m = re.exec(s); if (m) { parts.push(s.slice(start, i)); start = i + m[0].length; i = start - 1 } } } parts.push(s.slice(start)); return parts }
  const ok = (s: string): boolean => {
    s = strip(s)
    const ors = split(s, 'OR'); if (ors.length > 1) return ors.some(ok)
    const ands = split(s, 'AND'); if (ands.length > 1) return ands.every(ok)
    return ALLOWED.has(s)
  }
  return ok(e)
}

export function cargoComponents(meta: CargoMetadata, readTexts: (dir: string, extra?: string[]) => LicenseText[]): Component[] {
  const byId = new Map(meta.packages.map((p) => [p.id, p]))
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]))
  const seen = new Set<string>(), out: Component[] = []
  const walk = (id: string) => {
    if (seen.has(id)) return
    seen.add(id)
    const pkg = byId.get(id)!
    if (pkg.source !== null) {
      const license = LICENSE_OVERRIDES[pkg.name] ?? pkg.license
      out.push({ source: 'cargo', name: pkg.name, version: pkg.version, license, texts: readTexts(dirname(pkg.manifest_path), EXTRA_TEXTS[pkg.name]) })
    }
    for (const d of nodes.get(id)?.deps ?? []) if (d.dep_kinds.some((k) => k.kind === null)) walk(d.pkg)
  }
  if (meta.resolve.root) walk(meta.resolve.root)
  return out.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))
}

export function npmComponents(pkgs: NpmPackage[], readTexts: (dir: string) => LicenseText[]): Component[] {
  const out = new Map<string, Component>()
  for (const p of pkgs) { const k = `${p.name}@${p.version}`; if (!out.has(k)) out.set(k, { source: 'npm', name: p.name, version: p.version, license: p.license, texts: readTexts(p.dir) }) }
  return [...out.values()].sort((a, b) => a.name.localeCompare(b.name))
}

export function render(components: Component[]): string {
  const head = 'Aurora includes the following third-party software. Each component is listed with its license\nand the license texts its authors ship. Electron and Chromium licenses are in LICENSES.chromium.html\nnext to the Aurora executable.\n'
  const body = components.map((c) => [
    '='.repeat(78), `${c.name} ${c.version} (${c.source === 'cargo' ? 'Rust crate' : 'npm package'})`, `License: ${c.license ?? 'see text below'}`, '',
    ...c.texts.flatMap((t) => [`--- ${t.file} ---`, t.text.trim(), '']),
  ].join('\n'))
  return `${head}\n${body.join('\n')}\n`
}

export function readTexts(dir: string, extra: string[] = []): LicenseText[] {
  const files = existsSync(dir) ? readdirSync(dir).filter((f) => TEXT_FILE.test(f)).sort() : []
  return [...files, ...extra].filter((f) => existsSync(join(dir, f))).map((f) => ({ file: f, text: readFileSync(join(dir, f), 'utf8') }))
}

// ---- CLI ------------------------------------------------------------------------------------

function run(cmd: string, args: string[]): string {
  return execFileSync(cmd, args, { encoding: 'utf8', shell: process.platform === 'win32', maxBuffer: 256 << 20, cwd: REPO_ROOT, stdio: ['ignore', 'pipe', 'inherit'] })
}

function hostTriple(): string {
  const host = /^host:\s*(\S+)/m.exec(run('rustc', ['-vV']))?.[1]
  if (!host) throw new Error('could not read the host triple from `rustc -vV`')
  return host
}

function metadata(target: string, noFdk: boolean): CargoMetadata {
  const args = ['metadata', '--format-version', '1', '--locked', '--manifest-path', 'engine/Cargo.toml', '--filter-platform', target]
  if (noFdk) args.push('--no-default-features')
  const meta = JSON.parse(run('cargo', args)) as CargoMetadata
  // The workspace root is virtual, so cargo leaves resolve.root null: start from the engine itself.
  const engine = meta.packages.find((p) => p.name === 'aurora-engine' && p.source === null)
  if (!engine) throw new Error('aurora-engine is not in the cargo metadata')
  meta.resolve.root = engine.id
  return meta
}

/**
 * name@version of every crate cargo links into aurora-engine for this target, normal dependencies only.
 * `cargo metadata` resolves features for the whole workspace, so fixtures-gen (which turns the default
 * engine-core features on) keeps fdk-aac in the graph even with --no-default-features; `cargo tree -p` resolves
 * features for aurora-engine alone. cargoComponents takes licenses and paths from the metadata, filtered to this set.
 */
function linkedCrates(target: string, noFdk: boolean): Set<string> {
  const args = ['tree', '--locked', '--manifest-path', 'engine/Cargo.toml', '-p', 'aurora-engine', '--target', target, '-e', 'normal', '--prefix', 'none', '--format', '{p}']
  if (noFdk) args.push('--no-default-features')
  const out = new Set<string>()
  for (const line of run('cargo', args).split(/\r?\n/)) { const m = /^(\S+) v(\S+)/.exec(line); if (m) out.add(`${m[1]}@${m[2]}`) }
  return out
}

function readJson<T>(path: string): T { return JSON.parse(readFileSync(path, 'utf8')) as T }

/** The package `name` as seen from `parentDir`: its own nested node_modules first, then app/node_modules. */
function resolvePackage(name: string, parentDir: string | null, appDir: string): NpmPackage & { dependencies: string[] } {
  const candidates = [...(parentDir ? [join(parentDir, 'node_modules', name)] : []), join(appDir, 'node_modules', name)]
  const dir = candidates.find((d) => existsSync(join(d, 'package.json')))
  if (!dir) throw new Error(`cannot find npm package ${name} (looked in ${candidates.join(', ')})`)
  const json = readJson<{ version: string; license?: unknown; dependencies?: Record<string, string> }>(join(dir, 'package.json'))
  return { name, version: json.version, license: typeof json.license === 'string' ? json.license : null, dir, dependencies: Object.keys(json.dependencies ?? {}) }
}

function npmPackages(): NpmPackage[] {
  const appDir = join(REPO_ROOT, 'app')
  const lists = ['main', 'preload', 'renderer'].map((t) => join(appDir, `build/generated/bundled-${t}.json`)).filter(existsSync)
  if (lists.length === 0) throw new Error('no bundled-*.json found: run `npm --prefix app run build` first')
  const out = new Map<string, NpmPackage>()
  for (const list of lists) for (const p of readJson<NpmPackage[]>(list)) out.set(p.dir, { name: p.name, version: p.version, license: p.license, dir: p.dir })
  // Runtime dependencies electron-builder packs: the transitive closure of package.json `dependencies`.
  const seen = new Set<string>()
  const walk = (name: string, parentDir: string | null) => {
    const pkg = resolvePackage(name, parentDir, appDir)
    if (seen.has(pkg.dir)) return
    seen.add(pkg.dir)
    out.set(pkg.dir, { name: pkg.name, version: pkg.version, license: pkg.license, dir: pkg.dir })
    for (const dep of pkg.dependencies) walk(dep, pkg.dir)
  }
  for (const name of Object.keys(readJson<{ dependencies?: Record<string, string> }>(join(appDir, 'package.json')).dependencies ?? {})) walk(name, null)
  return [...out.values()]
}

function main(argv: string[]): number {
  const noFdk = argv.includes('--no-fdk')
  const targets: string[] = []
  let outPath = join(REPO_ROOT, 'app/build/generated/THIRD_PARTY_LICENSES.txt')
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--target') targets.push(argv[++i])
    else if (argv[i] === '--out') outPath = resolve(argv[++i])
  }
  if (targets.length === 0) targets.push(hostTriple())

  const merged = new Map<string, Component>()
  for (const t of targets) {
    const linked = linkedCrates(t, noFdk)
    for (const c of cargoComponents(metadata(t, noFdk), readTexts)) if (linked.has(`${c.name}@${c.version}`)) merged.set(`${c.name}@${c.version}`, c)
  }
  const cargo = [...merged.values()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))
  const npm = npmComponents(npmPackages(), (dir) => readTexts(dir))

  const problems: string[] = []
  for (const c of [...cargo, ...npm]) {
    if (c.license !== null && !licenseOk(c.license)) problems.push(`${c.name} ${c.version}: license "${c.license}" not allowed`)
    else if (c.license === null && c.texts.length === 0) problems.push(`${c.name} ${c.version}: no license text`)
  }
  for (const name of [...REQUIRED_TEXTS, ...(noFdk ? [] : REQUIRED_TEXTS_FDK)]) {
    const found = cargo.filter((c) => c.name === name)
    if (found.length === 0 || found.some((c) => c.texts.length === 0)) problems.push(`${name}: no license text`)
  }
  if (problems.length > 0) { for (const p of problems) console.error(p); return 1 }

  mkdirSync(dirname(outPath), { recursive: true })
  writeFileSync(outPath, render([...cargo, ...npm]))
  console.log(`licenses: ${cargo.length} Rust crates, ${npm.length} npm packages -> ${outPath}`)
  return 0
}

if (process.argv[1] === fileURLToPath(import.meta.url)) process.exit(main(process.argv.slice(2)))
