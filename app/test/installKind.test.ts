import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'
import { APP_ID } from '../src/main/appIdentity'
import { installKind } from '../src/main/installKind'
import { AUDIO_EXTENSIONS } from '../src/shared/api'

const base = { isPackaged: true, env: {}, exists: () => false }
describe('install kind', () => {
  it('is dev when unpackaged', () => {
    expect(installKind({ ...base, isPackaged: false, platform: 'win32', execPath: 'C:\\x\\electron.exe' })).toBe('dev')
  })
  it('tells an NSIS install from a portable zip by its uninstaller', () => {
    const exe = 'C:\\Users\\me\\AppData\\Local\\Programs\\Aurora\\Aurora.exe'
    const exists = (p: string) => p.replace(/\//g, '\\') === 'C:\\Users\\me\\AppData\\Local\\Programs\\Aurora\\Uninstall Aurora.exe'
    expect(installKind({ ...base, platform: 'win32', execPath: exe, exists })).toBe('nsis')
    expect(installKind({ ...base, platform: 'win32', execPath: 'D:\\Tools\\Aurora\\Aurora.exe' })).toBe('win-zip')
  })
  it('knows an AppImage from its environment and anything else on Linux as a package', () => {
    expect(installKind({ ...base, platform: 'linux', execPath: '/tmp/.mount_x/aurora', env: { APPIMAGE: '/home/me/Aurora.AppImage' } })).toBe('appimage')
    expect(installKind({ ...base, platform: 'linux', execPath: '/opt/Aurora/aurora' })).toBe('linux-package')
  })
  it('is mac on macOS', () => {
    expect(installKind({ ...base, platform: 'darwin', execPath: '/Applications/Aurora.app/Contents/MacOS/Aurora' })).toBe('mac')
  })
  it('matches electron-builder.yml', () => {
    const yml = readFileSync(join(__dirname, '../electron-builder.yml'), 'utf8')
    expect(yml).toContain(`appId: ${APP_ID}`)
    expect(yml).toMatch(/owner: kartikeychoudhary\s+repo: native-music-player/)
  })
  it('signs through the collect/apply hook, sha256 only, exe and dll, leaving the publisher to signed builds', () => {
    const yml = readFileSync(join(__dirname, '../electron-builder.yml'), 'utf8')
    expect(yml).toMatch(/signtoolOptions:\s+sign: build-tools\/sign\.cjs\s+signingHashAlgorithms: \[sha256\]/)
    expect(yml).toMatch(/signExts: \[\.exe, \.dll\]/)
    expect(yml).not.toContain('publisherName') // package-ci.ts sets it in the apply pass only
  })
  it('gives Linux packages the homepage and maintainer they require', () => {
    const pkg = JSON.parse(readFileSync(join(__dirname, '../package.json'), 'utf8')) as { homepage?: string; author?: { name?: string; email?: string } }
    expect(pkg.homepage?.startsWith('https://github.com/kartikeychoudhary/native-music-player')).toBe(true)
    expect(pkg.author?.name).toBeTruthy()
    expect(pkg.author?.email).toBeTruthy()
  })
  it('associates the audio extensions Aurora plays, except .mp4 (usually video), under its own class', () => {
    const yml = parse(readFileSync(join(__dirname, '../electron-builder.yml'), 'utf8')) as { fileAssociations?: { ext: string[]; name?: string; role?: string; mimeType?: unknown }[] }
    expect(yml.fileAssociations).toHaveLength(1)
    expect(yml.fileAssociations![0].ext).toEqual(AUDIO_EXTENSIONS.filter((e) => e !== 'mp4'))
    expect(yml.fileAssociations![0].role).toBe('Viewer')
    // The name becomes the Windows class HKCU\Software\Classes\<name>, which the uninstaller deletes whole.
    expect(yml.fileAssociations![0].name).toBe('Aurora.Audio')
  })
  it('gives Linux its MIME types through linux.mimeTypes, never the per-association string that writes malformed MIME XML', () => {
    const yml = parse(readFileSync(join(__dirname, '../electron-builder.yml'), 'utf8')) as { fileAssociations: { mimeType?: unknown }[]; linux: { mimeTypes?: unknown } }
    for (const a of yml.fileAssociations) expect(a).not.toHaveProperty('mimeType')
    expect(Array.isArray(yml.linux.mimeTypes)).toBe(true)
    expect(yml.linux.mimeTypes as string[]).toContain('audio/flac')
  })
  it('clears the dangling extension defaults on uninstall for exactly the associated extensions', () => {
    const yml = parse(readFileSync(join(__dirname, '../electron-builder.yml'), 'utf8')) as { fileAssociations: { ext: string[] }[] }
    const nsh = readFileSync(join(__dirname, '../build/installer.nsh'), 'utf8')
    const cleared = [...nsh.matchAll(/^\s*!insertmacro auroraClearDefault "([^"]+)"/gm)].map((m) => m[1])
    expect(new Set(cleared).size).toBe(cleared.length)
    expect([...cleared].sort()).toEqual([...yml.fileAssociations[0].ext].sort())
    // Only a real uninstall: an update re-registers the associations. Cleared before the context switch.
    const body = nsh.slice(nsh.indexOf('!macro customUnInstall'))
    expect(body.indexOf('${ifNot} ${isUpdated}')).toBeGreaterThanOrEqual(0)
    expect(body.indexOf('auroraClearDefault "mp3"')).toBeLessThan(body.indexOf('SetShellVarContext current'))
    // The shell context is switched only for a per-machine install, and always put back.
    expect(body.match(/SetShellVarContext current/g)).toHaveLength(1)
    expect(body.match(/SetShellVarContext all/g)).toHaveLength(1)
    expect(body.indexOf('SetShellVarContext all')).toBeGreaterThan(body.indexOf('RMDir /r'))
  })
  it('hands Linux file managers local paths (%F) rather than only URIs', () => {
    const yml = parse(readFileSync(join(__dirname, '../electron-builder.yml'), 'utf8')) as { linux: { executableArgs?: string[] } }
    expect(yml.linux.executableArgs).toEqual(['%F'])
  })
})
