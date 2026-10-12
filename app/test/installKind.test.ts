import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { APP_ID } from '../src/main/appIdentity'
import { installKind } from '../src/main/installKind'

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
  it('gives Linux packages the homepage and maintainer they require', () => {
    const pkg = JSON.parse(readFileSync(join(__dirname, '../package.json'), 'utf8')) as { homepage?: string; author?: { name?: string; email?: string } }
    expect(pkg.homepage?.startsWith('https://github.com/kartikeychoudhary/native-music-player')).toBe(true)
    expect(pkg.author?.name).toBeTruthy()
    expect(pkg.author?.email).toBeTruthy()
  })
})
