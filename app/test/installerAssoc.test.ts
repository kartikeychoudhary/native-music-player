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
    const writes = nsh.split('\n').filter((l) => /^\s*WriteReg\w+\s/.test(l))
    expect(writes.length).toBeGreaterThan(8)
    for (const l of writes) {
      // A write to Software\Classes\.<ext> itself (not a subkey) with the default value name "".
      expect(l, l).not.toMatch(/Software\\Classes\\\.[^"\\]*"\s+""/)
    }
    // The regex above must really catch what electron-builder's APP_ASSOCIATE writes.
    expect('WriteRegStr SHELL_CONTEXT "Software\\Classes\\.${EXT}" "" "Aurora.Audio"').toMatch(/Software\\Classes\\\.[^"\\]*"\s+""/)
    expect('WriteRegStr SHELL_CONTEXT "Software\\Classes\\.mp3" "" "Aurora.Audio"').toMatch(/Software\\Classes\\\.[^"\\]*"\s+""/)
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
  it('removes exactly what it added on a real uninstall only, and still clears a default that is ours', () => {
    const body = macroBody('customUnInstall')
    expect(body.indexOf('${ifNot} ${isUpdated}')).toBeGreaterThanOrEqual(0)
    expect(macroBody('auroraUnregisterExt')).toMatch(/DeleteRegValue SHELL_CONTEXT "Software\\Classes\\\.\$\{EXT\}\\OpenWithProgids" "Aurora\.Audio"/)
    // An OpenWithProgids key that is left empty (Aurora created it) goes too; one with values never does,
    // and the extension key itself is never deleted.
    const un = macroBody('auroraUnregisterExt')
    expect(un).toMatch(/EnumRegValue \$R1 SHELL_CONTEXT "Software\\Classes\\\.\$\{EXT\}\\OpenWithProgids" 0/)
    expect(un).toMatch(/EnumRegKey \$R2 SHELL_CONTEXT "Software\\Classes\\\.\$\{EXT\}\\OpenWithProgids" 0/)
    expect(un.indexOf('EnumRegValue')).toBeGreaterThan(un.indexOf('DeleteRegValue'))
    expect(un.indexOf('DeleteRegKey SHELL_CONTEXT "Software\\Classes\\.${EXT}\\OpenWithProgids"')).toBeGreaterThan(un.indexOf('EnumRegKey'))
    expect(un).not.toMatch(/DeleteRegKey SHELL_CONTEXT "Software\\Classes\\\.\$\{EXT\}"/)
    expect(body).toContain('DeleteRegKey SHELL_CONTEXT "Software\\Classes\\Aurora.Audio"')
    expect(body).toMatch(/DeleteRegKey SHELL_CONTEXT "Software\\Classes\\Applications\\\$\{APP_EXECUTABLE_FILENAME\}"/)
    expect(body).toContain('DeleteRegKey SHELL_CONTEXT "Software\\Clients\\Media\\Aurora"')
    expect(body).toMatch(/DeleteRegValue SHELL_CONTEXT "Software\\RegisteredApplications" "Aurora"/)
    expect(body).toContain('SHChangeNotify')
    // The default is only cleared while it is still Aurora.Audio (left by earlier test builds).
    expect(macroBody('auroraClearDefault')).toMatch(/\$\{if\} \$R0 == "Aurora\.Audio"/)
    // All of it is in the install's own context, before the one switch to the current user.
    expect(body.indexOf('auroraClearDefault')).toBeLessThan(body.indexOf('SetShellVarContext current'))
    expect(body.indexOf('RegisteredApplications')).toBeLessThan(body.indexOf('SetShellVarContext current'))
    expect(body.match(/SetShellVarContext current/g)).toHaveLength(1)
    expect(body.match(/SetShellVarContext all/g)).toHaveLength(1)
    expect(body.indexOf('SetShellVarContext all')).toBeGreaterThan(body.indexOf('RMDir /r'))
  })
})
