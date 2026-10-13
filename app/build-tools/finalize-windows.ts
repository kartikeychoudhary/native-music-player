// After SignPath signs the installer its bytes differ from what electron-builder hashed into latest.yml and the
// blockmap. This rewrites both from the signed file, so electron-updater's checksum and differential download hold.
import { createHash } from 'node:crypto'
import { createReadStream, existsSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { isMap, isScalar, isSeq, parseDocument, type YAMLMap } from 'yaml'

export type FileInfo = { sha512: string; size: number; blockMapSize?: number }

function setScalar(map: YAMLMap, key: string, value: string | number): void {
  const node = map.get(key, true)
  if (isScalar(node)) node.value = value // keeps the node's position and formatting
  else map.set(key, value)
}

/** Rewrites latest.yml in place: the matching `files[]` entries and the top-level sha512, leaving all else as it was. */
export function updateLatestYml(text: string, infos: Record<string, FileInfo>): string {
  const doc = parseDocument(text)
  const files = doc.get('files')
  if (!isSeq(files)) throw new Error('latest.yml has no files list')
  const entries = files.items.filter(isMap)
  const listed = new Set(entries.map((e) => e.get('url')))
  for (const name of Object.keys(infos)) {
    if (!listed.has(name)) throw new Error(`latest.yml does not list ${name}`)
  }
  for (const entry of entries) {
    const info = infos[String(entry.get('url'))]
    if (!info) continue
    setScalar(entry, 'sha512', info.sha512)
    setScalar(entry, 'size', info.size)
    if (info.blockMapSize !== undefined && entry.has('blockMapSize')) setScalar(entry, 'blockMapSize', info.blockMapSize)
  }
  // The top-level path is the installer electron-updater downloads. Leaving its sha512 stale breaks every update.
  const path = String(doc.get('path'))
  const top = infos[path]
  if (!top) throw new Error(`latest.yml path ${path} is not among the files to update`)
  doc.set('sha512', top.sha512)
  if (top.blockMapSize !== undefined && doc.has('blockMapSize')) doc.set('blockMapSize', top.blockMapSize)
  return String(doc)
}

function sha512Of(file: string): Promise<string> {
  return new Promise((res, rej) => {
    const h = createHash('sha512')
    createReadStream(file).on('data', (c) => h.update(c)).on('end', () => res(h.digest('base64'))).on('error', rej)
  })
}

async function finalize(dist: string): Promise<void> {
  const ymlPath = join(dist, 'latest.yml')
  const text = readFileSync(ymlPath, 'utf8')
  const ymlDoc = parseDocument(text)
  const files = ymlDoc.get('files')
  if (!isSeq(files)) throw new Error('latest.yml has no files list')
  // electron-builder 26 ships buildBlockMap as CommonJS inside app-builder-lib; it is not part of a public entry point.
  const { buildBlockMap } = createRequire(import.meta.url)('app-builder-lib/out/targets/blockmap/blockmap') as {
    buildBlockMap: (file: string, compression: 'gzip' | 'deflate', outFile?: string) => Promise<FileInfo>
  }
  const infos: Record<string, FileInfo> = {}
  for (const entry of files.items.filter(isMap)) {
    const url = String(entry.get('url'))
    const file = join(dist, url)
    if (!existsSync(file)) {
      console.log(`skip ${url}: not in ${dist}`)
      continue
    }
    const old = String(entry.get('sha512'))
    const info = existsSync(`${file}.blockmap`)
      ? await buildBlockMap(file, 'gzip', `${file}.blockmap`)
      : { sha512: await sha512Of(file), size: statSync(file).size }
    infos[url] = info
    console.log(`${url}: sha512 ${old.slice(0, 12)} -> ${info.sha512.slice(0, 12)}${existsSync(`${file}.blockmap`) ? ' (blockmap rebuilt)' : ''}`)
  }
  const installer = String(ymlDoc.get('path'))
  if (!infos[installer]) throw new Error(`the installer ${installer} named by latest.yml is not in ${dist}; was it renamed by signing?`)
  writeFileSync(ymlPath, updateLatestYml(text, infos))
  console.log(`rewrote ${ymlPath}`)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const i = process.argv.indexOf('--dist')
  if (i < 0 || !process.argv[i + 1]) {
    console.error('usage: node finalize-windows.ts --dist <dir>')
    process.exit(2)
  }
  finalize(resolve(process.argv[i + 1])).catch((e) => {
    console.error(e)
    process.exit(1)
  })
}
