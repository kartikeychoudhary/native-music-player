import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

/** The repository root (app/build-tools/../..). */
export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')

export const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/

const SECTION = /^\[workspace\.package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m
const VERSION = /^version\s*=\s*"([^"]*)"/m

export function engineVersion(cargoToml: string): string | null {
  const section = SECTION.exec(cargoToml)?.[1]
  return section ? VERSION.exec(section)?.[1] ?? null : null
}

export function setEngineVersion(cargoToml: string, v: string): string {
  return cargoToml.replace(SECTION, (section) => section.replace(VERSION, `version = "${v}"`))
}

export function checkVersions(o: { app: string; engine: string | null; tag?: string }): string[] {
  const problems: string[] = []
  if (!SEMVER.test(o.app)) problems.push(`app/package.json version "${o.app}" is not semver`)
  if (o.engine === null) problems.push('engine/Cargo.toml has no [workspace.package] version')
  else if (o.engine !== o.app) problems.push(`app/package.json has ${o.app} but engine/Cargo.toml has ${o.engine}`)
  if (o.tag !== undefined && o.tag !== `v${o.app}`) problems.push(`tag ${o.tag} does not match version ${o.app}`)
  return problems
}
