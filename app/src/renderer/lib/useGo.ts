import { useRouter } from '@tanstack/react-router'
import { useCallback } from 'react'

/** Navigates to a concrete path such as `/albums/12` (menus and cards build paths, not route ids). */
export function useGo() {
  const router = useRouter()
  return useCallback((path: string) => { void router.navigate({ to: path as never }) }, [router])
}
