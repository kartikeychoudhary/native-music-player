export type ArtSize = 64 | 300 | 1000
/** URL served by the main process's `aurora-art://` handler (which asks the engine for the cache file). */
export const artUrl = (id: number, size: ArtSize): string => `aurora-art://art/${id}/${size}`
