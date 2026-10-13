import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'
import { AUDIO_EXTENSIONS } from '../src/shared/api'

type Yml = {
  fileAssociations?: unknown
  linux: { mimeTypes?: unknown }
  mac: { fileAssociations?: { ext: string[]; name?: string; role?: string; mimeType?: unknown }[] }
}
const yml = parse(readFileSync(join(__dirname, '../electron-builder.yml'), 'utf8')) as Yml
const nsh = readFileSync(join(__dirname, '../build/installer.nsh'), 'utf8')
const EXTS = AUDIO_EXTENSIONS.filter((e) => e !== 'mp4')

// Text of one `!macro NAME ... !macroend` block.
const macroBody = (name: string): string => {
  const start = nsh.indexOf(`!macro ${name}`)
  expect(start, `!macro ${name} exists`).toBeGreaterThanOrEqual(0)
  return nsh.slice(start, nsh.indexOf('!macroend', start))
}

describe('Windows file types are registered by installer.nsh, never by electron-builder', () => {
  it('keeps electron-builder file associations for macOS only', () => {
    // A top-level fileAssociations also feeds NSIS APP_ASSOCIATE, which writes each .ext default.
    expect(yml.fileAssociations).toBeUndefined()
    expect(yml.mac.fileAssociations).toHaveLength(1)
    const a = yml.mac.fileAssociations![0]
    expect(a.ext).toEqual(EXTS)
    expect(a.name).toBe('Aurora.Audio')
    expect(a.role).toBe('Viewer')
    expect(a).not.toHaveProperty('mimeType')
  })
  it('gives Linux its MIME types through linux.mimeTypes', () => {
    expect(Array.isArray(yml.linux.mimeTypes)).toBe(true)
    expect(yml.linux.mimeTypes as string[]).toContain('audio/flac')
  })
  it('has one extension list, equal to the macOS one, used for install and uninstall', () => {
    const list = macroBody('auroraForEachExt')
    const exts = [...list.matchAll(/^\s*!insertmacro "\$\{ACTION\}" "([^"]+)"/gm)].map((m) => m[1])
    expect(new Set(exts).size).toBe(exts.length)
    expect([...exts].sort()).toEqual([...EXTS].sort())
    expect(macroBody('customInstall')).toMatch(/!insertmacro auroraForEachExt auroraRegisterExt/)
    expect(macroBody('customUnInstall')).toMatch(/!insertmacro auroraForEachExt auroraUnregisterExt/)
    expect(macroBody('customUnInstall')).toMatch(/!insertmacro auroraForEachExt auroraClearDefault/)
    // No extension is hard-coded anywhere else.
    expect(nsh.replace(list, '')).not.toMatch(/\.(mp3|flac|ogg|opus|m4a)\b/)
  })
  it("never writes an extension default value, which would take over the user's default player", () => {
    const writes = nsh.split('\n').filter((l) => /^\s*WriteReg\w+\s/i.test(l))
    expect(writes.length).toBeGreaterThan(8)
    for (const l of writes) {
      // A write to Software\Classes\.<ext> itself (not a subkey) with the default value name "".
      expect(l, l).not.toMatch(/Software\\Classes\\\.[^"\\]*"\s+""/i)
    }
    // The regex above must really catch what electron-builder's APP_ASSOCIATE writes, in any case.
    expect('WriteRegStr SHELL_CONTEXT "Software\\Classes\\.${EXT}" "" "Aurora.Audio"').toMatch(/Software\\Classes\\\.[^"\\]*"\s+""/i)
    expect('WriteRegStr SHELL_CONTEXT "Software\\Classes\\.mp3" "" "Aurora.Audio"').toMatch(/Software\\Classes\\\.[^"\\]*"\s+""/i)
    expect('writeregstr shell_context "SOFTWARE\\CLASSES\\.FLAC" "" "x"').toMatch(/Software\\Classes\\\.[^"\\]*"\s+""/i)
    // And no use of electron-builder's macros that do (comments may name them).
    expect(nsh.replace(/^\s*;.*$/gm, '')).not.toMatch(/APP_ASSOCIATE/)
  })
  it('registers Open with, the application, and the Default apps capabilities on install', () => {
    const body = macroBody('customInstall')
    const ext = macroBody('auroraRegisterExt')
    expect(ext).toMatch(/WriteRegNone SHELL_CONTEXT "Software\\Classes\\\.\$\{EXT\}\\OpenWithProgids" "Aurora\.Audio"/)
    expect(ext).toMatch(/Applications\\\$\{APP_EXECUTABLE_FILENAME\}\\SupportedTypes" "\.\$\{EXT\}" ""/)
    expect(ext).toMatch(/Clients\\Media\\Aurora\\Capabilities\\FileAssociations" "\.\$\{EXT\}" "Aurora\.Audio"/)
    expect(body).toContain('Software\\Classes\\Aurora.Audio')
    expect(body).toContain('Software\\Clients\\Media\\Aurora\\Capabilities')
    expect(body).toMatch(/"ApplicationName" "Aurora"/)
    expect(body).toMatch(/Software\\RegisteredApplications" "Aurora" "Software\\Clients\\Media\\Aurora\\Capabilities"/)
    expect(body).toMatch(/shell\\open\\command" "" `"\$appExe" "%1"`/)
    expect(body).toContain('SHChangeNotify')
  })
  it('removes its registrations on every uninstall, including the one an update or install-mode switch runs', () => {
    const body = macroBody('customUnInstall')
    const un = macroBody('auroraUnregisterExt')
    expect(un).toMatch(/DeleteRegValue SHELL_CONTEXT "Software\\Classes\\\.\$\{EXT\}\\OpenWithProgids" "Aurora\.Audio"/)
    // An OpenWithProgids key, then the extension key, that this leaves empty goes too (no values, no
    // subkeys, no default); one that still holds anything stays.
    expect(un).toMatch(/auroraDeleteKeyIfEmpty "Software\\Classes\\\.\$\{EXT\}\\OpenWithProgids"/)
    expect(un).toMatch(/auroraDeleteKeyIfEmpty "Software\\Classes\\\.\$\{EXT\}"/)
    expect(un.indexOf('DeleteRegValue')).toBeLessThan(un.indexOf('auroraDeleteKeyIfEmpty'))
    expect(un.indexOf('auroraDeleteKeyIfEmpty "Software\\Classes\\.${EXT}\\OpenWithProgids"')).toBeLessThan(un.indexOf('auroraDeleteKeyIfEmpty "Software\\Classes\\.${EXT}"'))
    expect(un).not.toMatch(/DeleteRegKey/)
    const empty = macroBody('auroraDeleteKeyIfEmpty')
    expect(empty).toMatch(/EnumRegValue \$R1 SHELL_CONTEXT "\$\{KEY\}" 0/)
    expect(empty).toMatch(/EnumRegKey \$R2 SHELL_CONTEXT "\$\{KEY\}" 0/)
    expect(empty).toMatch(/ReadRegStr \$R3 SHELL_CONTEXT "\$\{KEY\}" ""/)
    // A second value (index 1) must be absent too: index 0 may be an unnamed empty (Default) with named
    // values such as Content Type after it, and a key holding those must never be deleted.
    expect(empty).toMatch(/EnumRegValue \$R4 SHELL_CONTEXT "\$\{KEY\}" 1/)
    expect(empty).toMatch(/\$R1 == ""\s+\$\{andIf\} \$R4 == ""\s+\$\{andIf\} \$R2 == ""\s+\$\{andIf\} \$R3 == ""\s+DeleteRegKey SHELL_CONTEXT "\$\{KEY\}"/)

    // Placement, by order: [gate 1: clear a default that is ours] [removals] [gate 2: the data prompt].
    const gate = '${ifNot} ${isUpdated}'
    const g1 = body.indexOf(gate)
    const g1End = body.indexOf('${endIf}', g1)
    const g2 = body.indexOf(gate, g1End)
    expect(body.match(/\$\{isUpdated\}/g)).toHaveLength(2)
    expect(g1).toBeGreaterThanOrEqual(0)
    const clear = body.indexOf('auroraClearDefault')
    expect(clear).toBeGreaterThan(g1)
    expect(clear).toBeLessThan(g1End)
    const removals = [
      'auroraUnregisterExt',
      'DeleteRegKey SHELL_CONTEXT "Software\\Classes\\Aurora.Audio"',
      'DeleteRegKey SHELL_CONTEXT "Software\\Classes\\Applications\\${APP_EXECUTABLE_FILENAME}"',
      'DeleteRegKey SHELL_CONTEXT "Software\\Clients\\Media\\Aurora"',
      'DeleteRegValue SHELL_CONTEXT "Software\\RegisteredApplications" "Aurora"',
      'SHChangeNotify'
    ]
    for (const r of removals) {
      const at = body.indexOf(r)
      expect(at, r).toBeGreaterThan(g1End) // not inside gate 1 ...
      expect(at, r).toBeLessThan(g2) // ... nor gate 2, so an update or a mode switch removes them
    }
    // The prompt and its deletion are in gate 2, after every removal.
    expect(body.indexOf('MessageBox')).toBeGreaterThan(g2)
    expect(body.indexOf('RMDir /r')).toBeGreaterThan(g2)
    // The default is only cleared while it is still Aurora.Audio (left by earlier test builds).
    expect(macroBody('auroraClearDefault')).toMatch(/\$\{if\} \$R0 == "Aurora\.Audio"/)
    // Everything is in the install's own context, before the one switch to the current user.
    expect(body.indexOf('SetShellVarContext current')).toBeGreaterThan(g2)
    expect(body.match(/SetShellVarContext current/g)).toHaveLength(1)
    expect(body.match(/SetShellVarContext all/g)).toHaveLength(1)
    expect(body.indexOf('SetShellVarContext all')).toBeGreaterThan(body.indexOf('RMDir /r'))
  })
})
