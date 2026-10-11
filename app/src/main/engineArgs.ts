/** Where the engine keeps library.db, artwork/ and session.json. Tests may redirect it (unpackaged only). */
export function resolveDataDir(o: { isPackaged: boolean; env: NodeJS.ProcessEnv | Record<string, string | undefined>; userData: string }): string {
  return (!o.isPackaged && o.env.AURORA_DATA_DIR) || o.userData
}

export function engineArgs(dataDir: string): string[] {
  return ['rpc', '--data-dir', dataDir]
}
