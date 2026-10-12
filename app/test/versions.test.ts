import { describe, expect, it } from 'vitest'
import { checkVersions, engineVersion, setEngineVersion } from '../build-tools/versions.ts'

const toml = `[workspace]\nmembers = ["crates/x"]\n\n[workspace.package]\nedition = "2024"\nversion = "0.1.0"\nrust-version = "1.88"\n\n[workspace.dependencies]\nserde = { version = "1" }\n`

describe('versions', () => {
  it('reads the workspace version, not a dependency version', () => {
    expect(engineVersion(toml)).toBe('0.1.0')
    expect(engineVersion('[workspace.dependencies]\nserde = { version = "1" }\n')).toBeNull()
  })
  it('rewrites only the workspace version', () => {
    const next = setEngineVersion(toml, '0.2.0-rc.1')
    expect(engineVersion(next)).toBe('0.2.0-rc.1')
    expect(next).toContain('serde = { version = "1" }')
    expect(next.replace('0.2.0-rc.1', '0.1.0')).toBe(toml)
  })
  it('reports every mismatch', () => {
    expect(checkVersions({ app: '0.1.0', engine: '0.1.0' })).toEqual([])
    expect(checkVersions({ app: '0.1.0', engine: '0.1.0', tag: 'v0.1.0' })).toEqual([])
    expect(checkVersions({ app: '0.1.0', engine: '0.2.0' })).toEqual(['app/package.json has 0.1.0 but engine/Cargo.toml has 0.2.0'])
    expect(checkVersions({ app: '0.1.0', engine: '0.1.0', tag: 'v0.1.1' })).toEqual(['tag v0.1.1 does not match version 0.1.0'])
    expect(checkVersions({ app: '1.0', engine: null })).toEqual(['app/package.json version "1.0" is not semver', 'engine/Cargo.toml has no [workspace.package] version'])
  })
})
