import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'

type Step = { uses?: string; run?: string; with?: Record<string, unknown>; env?: Record<string, string>; name?: string }
type Job = { needs?: string | string[]; permissions?: Record<string, string>; strategy?: { matrix: { include: Record<string, string>[] } }; steps: Step[]; if?: string }
const wf = parse(readFileSync(join(__dirname, '../../.github/workflows/release.yml'), 'utf8')) as { on: Record<string, unknown>; permissions: Record<string, string>; jobs: Record<string, Job> }

describe('release workflow', () => {
  it('runs on v* tags and by hand', () => {
    expect((wf.on.push as { tags: string[] }).tags).toEqual(['v*'])
    expect(wf.on).toHaveProperty('workflow_dispatch')
  })
  it('builds the engine for exactly the six targets', () => {
    const dirs = wf.jobs.engine.strategy!.matrix.include.map((m) => m.dir).sort()
    expect(dirs).toEqual(['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-arm64', 'win32-x64'])
  })
  it('hands engines to packaging under matching artifact names', () => {
    const up = wf.jobs.engine.steps.find((s) => s.uses?.startsWith('actions/upload-artifact'))!
    const down = wf.jobs.package.steps.find((s) => s.uses?.startsWith('actions/download-artifact'))!
    expect(up.with!.name).toBe('engine-${{ matrix.dir }}')
    expect(down.with!.pattern).toBe('engine-${{ matrix.platform }}-*')
    expect(wf.jobs.engine.steps.some((s) => s.run?.includes('tar -cf'))).toBe(true) // keeps the exec bit
  })
  it('only the release job can write, and it creates a draft', () => {
    expect(wf.permissions).toEqual({ contents: 'read' })
    for (const [name, job] of Object.entries(wf.jobs)) if (name !== 'release') expect(job.permissions?.contents ?? 'read').toBe('read')
    expect(wf.jobs.release.permissions).toEqual({ contents: 'write' })
    expect(wf.jobs.release.steps.some((s) => s.run?.includes('gh release create') && s.run.includes('--draft'))).toBe(true)
  })
  it('every needs names a job, and every action is pinned to a version', () => {
    for (const job of Object.values(wf.jobs)) for (const n of [job.needs ?? []].flat()) expect(Object.keys(wf.jobs)).toContain(n)
    for (const job of Object.values(wf.jobs)) for (const s of job.steps) if (s.uses) expect(s.uses).toMatch(/@v?\d/)
  })
  it('checks linked libraries and the handshake of every engine, and smoke-tests every package job', () => {
    expect(wf.jobs.engine.steps.some((s) => s.run?.includes('check-engine-deps.sh'))).toBe(true)
    expect(wf.jobs.engine.steps.some((s) => s.run?.includes('engine.ready'))).toBe(true)
    expect(wf.jobs.package.steps.some((s) => s.run?.includes('e2e:packaged'))).toBe(true)
  })
  it('does not fail the Linux package job on the AppArmor sysctl', () => {
    const sysctl = wf.jobs.package.steps.flatMap((s) => s.run?.split(/\r?\n/) ?? []).filter((l) => l.includes('apparmor_restrict_unprivileged_userns'))
    expect(sysctl).toHaveLength(1)
    expect(sysctl[0]).toMatch(/\|\| true\s*$/)
  })
  it('also smoke-tests the no-fdk package, from its own unpacked app, on every OS', () => {
    const matrix = wf.jobs.package.strategy!.matrix.include
    for (const m of matrix) expect(m.exe_nofdk).toMatch(/^dist-nofdk\//)
    const smokes = wf.jobs.package.steps.filter((s) => s.env?.AURORA_EXPECT_VARIANT === 'nofdk')
    expect(smokes.length).toBeGreaterThanOrEqual(2) // Linux under xvfb, Windows and macOS without
    for (const s of smokes) {
      expect(s.run).toContain('e2e:packaged')
      expect(s.env!.AURORA_PACKAGED_EXE).toBe('${{ matrix.exe_nofdk }}')
    }
    expect(smokes.some((s) => s.run!.startsWith('xvfb-run -a '))).toBe(true)
  })
  it('signs Windows through SignPath only when configured, with the binaries then the installer', () => {
    const steps = wf.jobs.package.steps
    const signpath = steps.filter((s) => s.uses?.startsWith('signpath/github-action-submit-signing-request@'))
    expect(signpath.map((s) => s.with!['artifact-configuration-slug'])).toEqual(['binaries', 'installer'])
    for (const s of signpath) {
      expect((s as { if?: string }).if).toContain("vars.SIGNPATH_ORGANIZATION_ID != ''")
      expect(s.with!['signing-policy-slug']).toBe("${{ github.event_name == 'push' && 'release-signing' || 'test-signing' }}")
      expect(s.with!['api-token']).toBe('${{ secrets.SIGNPATH_API_TOKEN }}')
      expect(s.with!['wait-for-completion']).toBe(true)
    }
    const unsigned = steps.find((s) => s.name === 'Package')! as { if?: string }
    expect(unsigned.if).toContain("vars.SIGNPATH_ORGANIZATION_ID == ''")
  })
  it('finalizes latest.yml after the installer is signed and checks the publisher on release builds', () => {
    const runs = wf.jobs.package.steps.map((s) => s.run ?? '').join('\n')
    expect(runs).toContain('finalize-windows.ts')
    expect(runs).toMatch(/check-signatures\.ps1[^\n]*-Publisher 'SignPath Foundation'/)
    const order = (needle: string) => wf.jobs.package.steps.findIndex((s) => (s.run ?? '').includes(needle) || JSON.stringify(s.with ?? {}).includes(needle))
    expect(order('--sign-mode collect')).toBeLessThan(order('binaries'))
    expect(order('binaries')).toBeLessThan(order('--sign-mode apply'))
    expect(order('--sign-mode apply')).toBeLessThan(order('"installer"'))
    expect(order('"installer"')).toBeLessThan(order('finalize-windows.ts'))
  })
  it('keeps the artifact configurations next to the workflow', () => {
    for (const slug of ['binaries', 'installer']) expect(existsSync(join(__dirname, `../../.signpath/artifact-configurations/${slug}.xml`))).toBe(true)
  })
  it('signs Windows with SignPath only; the Package step carries no Windows certificate secrets', () => {
    const pkg = wf.jobs.package.steps.find((s) => s.name === 'Package')!
    expect(JSON.stringify(pkg.env)).not.toContain('WIN_CSC')
  })
})
