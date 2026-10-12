import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { Plugin } from 'vite'

/** The npm package a bundled module id belongs to (the innermost node_modules entry), or null. */
export function packageOf(id: string): { name: string; dir: string } | null {
  const p = id.replace(/\\/g, '/')
  const at = p.lastIndexOf('/node_modules/')
  if (id.startsWith('\0') || at < 0) return null
  const rest = p.slice(at + '/node_modules/'.length).split('/')
  const name = rest[0].startsWith('@') ? `${rest[0]}/${rest[1]}` : rest[0]
  return { name, dir: `${p.slice(0, at)}/node_modules/${name}` }
}

/**
 * Records which npm packages ended up in this build (for THIRD_PARTY_LICENSES.txt), as
 * `<outDir>/bundled-<target>.json`. outDir is passed in rather than derived from this file's
 * location because electron-vite bundles the config to a temporary file.
 */
export function bundledPackages(target: 'main' | 'preload' | 'renderer', outDir: string): Plugin {
  return {
    name: 'aurora-bundled-packages',
    apply: 'build',
    generateBundle() {
      const seen = new Map<string, { name: string; version: string; license: string | null; dir: string }>()
      for (const id of this.getModuleIds()) {
        const pkg = packageOf(id)
        if (!pkg || seen.has(pkg.dir)) continue
        const json = JSON.parse(readFileSync(join(pkg.dir, 'package.json'), 'utf8')) as { version: string; license?: unknown }
        seen.set(pkg.dir, { ...pkg, version: json.version, license: typeof json.license === 'string' ? json.license : null })
      }
      mkdirSync(outDir, { recursive: true })
      writeFileSync(join(outDir, `bundled-${target}.json`), JSON.stringify([...seen.values()].sort((a, b) => a.name.localeCompare(b.name)), null, 2))
    },
  }
}
