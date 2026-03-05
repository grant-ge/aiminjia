/**
 * useTauriEvent — Generic typed Tauri event listener hook.
 *
 * Registers a Tauri event listener on mount and automatically
 * unlistens on unmount to prevent memory leaks.
 */
import { useEffect } from 'react'

/**
 * Listen to a Tauri event with automatic cleanup on unmount.
 *
 * The `setup` function should call one of the Tauri event listener
 * helpers (e.g. `onStreamingDelta`) and return the unlisten function
 * that Tauri provides.
 *
 * @param setup - Async function that registers the listener and returns
 *                an `unlisten` callback.
 *
 * @example
 * ```ts
 * useTauriEvent(() =>
 *   onStreamingDelta(({ delta }) => {
 *     store.appendStreamingContent(delta)
 *   })
 * )
 * ```
 */
export function useTauriEvent(setup: () => Promise<() => void>) {
  useEffect(() => {
    let unlisten: (() => void) | undefined
    let mounted = true

    setup().then((fn) => {
      if (mounted) {
        unlisten = fn
      } else {
        // Component already unmounted before the listener was ready — clean up immediately
        fn()
      }
    })

    return () => {
      mounted = false
      unlisten?.()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []) // intentionally empty deps — setup should be a stable reference
}
