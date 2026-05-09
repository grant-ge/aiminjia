import { useEffect } from 'react'
import { useUpdaterStore } from '@/lib/updaterStore'

/**
 * Drives the updater state machine on mount. Reads pending.json, runs the
 * boot decision tree (§4 of the design doc), kicks off a background download
 * if appropriate, and surfaces phase/version/progress through the store.
 *
 * The component tree consumes `useUpdaterStore` directly for rendering — this
 * hook is the single bootstrap entry, called once near the app root.
 */
export function useUpdater(): void {
  useEffect(() => {
    void useUpdaterStore.getState().bootstrap()
  }, [])
}
