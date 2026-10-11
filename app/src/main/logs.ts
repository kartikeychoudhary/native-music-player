import { app } from 'electron'
import { appendFile, mkdirSync } from 'node:fs'
import { join } from 'node:path'

export const logDir = () => join(app.getPath('userData'), 'logs')

export function logger(file: string) {
  const dir = logDir()
  mkdirSync(dir, { recursive: true })
  const path = join(dir, file)
  return (line: string) => appendFile(path, `${new Date().toISOString()} ${line}\n`, () => {})
}
