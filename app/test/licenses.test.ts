import { describe, expect, it } from 'vitest'
import { packageOf } from '../build-tools/bundledPackages.ts'
import { cargoComponents, licenseOk, npmComponents, render, type CargoMetadata } from '../build-tools/licenses.ts'

describe('bundled package paths', () => {
  it('finds the package of a module id, scoped, nested, Windows or POSIX', () => {
    expect(packageOf('/r/app/node_modules/react/index.js')).toEqual({ name: 'react', dir: '/r/app/node_modules/react' })
    expect(packageOf('C:\\r\\app\\node_modules\\@tanstack\\react-router\\dist\\esm\\index.js')).toEqual({ name: '@tanstack/react-router', dir: 'C:/r/app/node_modules/@tanstack/react-router' })
    expect(packageOf('/r/node_modules/a/node_modules/b/x.js')).toEqual({ name: 'b', dir: '/r/node_modules/a/node_modules/b' })
    expect(packageOf('\0vite/preload-helper')).toBeNull()
    expect(packageOf('/r/app/src/main/index.ts')).toBeNull()
  })
})

describe('license expressions', () => {
  it('accepts allowed licenses, any OR branch, and every AND branch', () => {
    expect(licenseOk('MIT')).toBe(true)
    expect(licenseOk('MIT OR Apache-2.0')).toBe(true)
    expect(licenseOk('MIT/Apache-2.0')).toBe(true)
    expect(licenseOk('(MIT OR Apache-2.0) AND Unicode-3.0')).toBe(true)
    expect(licenseOk('Apache-2.0 WITH LLVM-exception')).toBe(true)
    expect(licenseOk('GPL-3.0-only OR MIT')).toBe(true)
  })
  it('rejects copyleft-only, unknown and missing licenses', () => {
    expect(licenseOk('GPL-3.0-only')).toBe(false)
    expect(licenseOk('MIT AND LGPL-2.1-or-later')).toBe(false)
    expect(licenseOk('SEE LICENSE IN LICENSE.md')).toBe(false)
    expect(licenseOk(null)).toBe(false)
  })
})

const meta: CargoMetadata = {
  packages: [
    { id: 'root', name: 'aurora-engine', version: '0.1.0', license: 'MIT', manifest_path: '/w/aurora-engine/Cargo.toml', source: null },
    { id: 'lib', name: 'library', version: '0.1.0', license: 'MIT', manifest_path: '/w/library/Cargo.toml', source: null },
    { id: 'a', name: 'a', version: '1.0.0', license: 'MIT OR Apache-2.0', manifest_path: '/reg/a-1.0.0/Cargo.toml', source: 'registry+https://github.com/rust-lang/crates.io-index' },
    { id: 'b', name: 'b', version: '2.0.0', license: null, manifest_path: '/reg/b-2.0.0/Cargo.toml', source: 'registry+x' },
    { id: 'dev', name: 'tempfile', version: '3.0.0', license: 'MIT', manifest_path: '/reg/tempfile/Cargo.toml', source: 'registry+x' },
    { id: 'bld', name: 'cc', version: '1.0.0', license: 'MIT', manifest_path: '/reg/cc/Cargo.toml', source: 'registry+x' },
  ],
  resolve: {
    root: 'root',
    nodes: [
      { id: 'root', deps: [{ pkg: 'lib', dep_kinds: [{ kind: null }] }, { pkg: 'dev', dep_kinds: [{ kind: 'dev' }] }] },
      { id: 'lib', deps: [{ pkg: 'a', dep_kinds: [{ kind: null }] }, { pkg: 'bld', dep_kinds: [{ kind: 'build' }] }] },
      { id: 'a', deps: [{ pkg: 'b', dep_kinds: [{ kind: null }] }] },
      { id: 'b', deps: [] }, { id: 'dev', deps: [] }, { id: 'bld', deps: [] },
    ],
  },
}

describe('components', () => {
  it('walks normal dependencies of the root, skipping our own crates, dev and build deps', () => {
    const texts = (dir: string) => (dir.endsWith('b-2.0.0') ? [{ file: 'LICENSE', text: 'B license' }] : [])
    const got = cargoComponents(meta, texts)
    expect(got.map((c) => `${c.name}@${c.version}`)).toEqual(['a@1.0.0', 'b@2.0.0'])
    expect(got[1]).toMatchObject({ license: null, texts: [{ file: 'LICENSE', text: 'B license' }] })
  })
  it('takes npm packages as given, deduplicated by name and version', () => {
    const got = npmComponents([{ name: 'react', version: '19.3.0', license: 'MIT', dir: '/n/react' }, { name: 'react', version: '19.3.0', license: 'MIT', dir: '/n/react' }], () => [{ file: 'LICENSE', text: 'MIT …' }])
    expect(got).toHaveLength(1)
  })
  it('renders each component with its license and texts', () => {
    const out = render([{ source: 'cargo', name: 'a', version: '1.0.0', license: 'MIT', texts: [{ file: 'LICENSE-MIT', text: 'Permission is hereby granted' }] }])
    expect(out).toContain('a 1.0.0 (Rust crate)')
    expect(out).toContain('License: MIT')
    expect(out).toContain('Permission is hereby granted')
  })
})
