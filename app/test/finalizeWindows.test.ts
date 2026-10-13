import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'
import { updateLatestYml } from '../build-tools/finalize-windows.ts'

const latest = `version: 0.1.0
files:
  - url: Aurora-0.1.0-win-setup.exe
    sha512: OLDSHA==
    size: 100
path: Aurora-0.1.0-win-setup.exe
sha512: OLDSHA==
releaseDate: '2026-10-13T08:00:00.000Z'
`

describe('finalize windows', () => {
  it('rewrites the signed installer hash and size everywhere it appears, keeping the rest', () => {
    const out = updateLatestYml(latest, { 'Aurora-0.1.0-win-setup.exe': { sha512: 'NEWSHA==', size: 123 } })
    const y = parse(out)
    expect(y.files[0]).toEqual({ url: 'Aurora-0.1.0-win-setup.exe', sha512: 'NEWSHA==', size: 123 })
    expect(y.sha512).toBe('NEWSHA==')
    expect(y.version).toBe('0.1.0')
    expect(y.releaseDate).toBe('2026-10-13T08:00:00.000Z')
    expect(out.indexOf('version')).toBeLessThan(out.indexOf('files'))
  })
  it('updates blockMapSize only when the entry had one', () => {
    const withMap = latest.replace('    size: 100\n', '    size: 100\n    blockMapSize: 7\n')
    expect(parse(updateLatestYml(withMap, { 'Aurora-0.1.0-win-setup.exe': { sha512: 'N==', size: 1, blockMapSize: 9 } })).files[0].blockMapSize).toBe(9)
    expect(parse(updateLatestYml(latest, { 'Aurora-0.1.0-win-setup.exe': { sha512: 'N==', size: 1, blockMapSize: 9 } })).files[0]).not.toHaveProperty('blockMapSize')
  })
  it('refuses a file latest.yml does not list', () => {
    expect(() => updateLatestYml(latest, { 'Other.exe': { sha512: 'N==', size: 1 } })).toThrow(/Other\.exe/)
  })
  it('refuses to finish when the installer named by path is not among the files', () => {
    const two = latest.replace('path: Aurora', '  - url: Aurora-0.1.0-win-x64.zip\n    sha512: ZIP==\n    size: 5\npath: Aurora')
    expect(() => updateLatestYml(two, { 'Aurora-0.1.0-win-x64.zip': { sha512: 'N==', size: 1 } })).toThrow(/Aurora-0\.1\.0-win-setup\.exe/)
  })
})
