import type { MouseEvent } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'

const INTERACTIVE_TARGET_SELECTOR = [
  'button',
  'a',
  'input',
  'textarea',
  'select',
  '[role="button"]',
  '[role="link"]',
  '[role="menuitem"]',
  '[contenteditable="true"]',
  '[data-aijia-window-drag-exempt]',
].join(',')

function isInteractiveTarget(target: EventTarget | null, currentTarget: HTMLElement) {
  if (!(target instanceof Element)) return false
  const interactive = target.closest(INTERACTIVE_TARGET_SELECTOR)
  return Boolean(interactive && currentTarget.contains(interactive))
}

export function handleChromeDragRegionMouseDown(event: MouseEvent<HTMLElement>) {
  const userAgent = navigator.userAgent
  if (!userAgent.includes('Windows') && !userAgent.includes('Macintosh')) return
  if (event.button !== 0 || event.buttons !== 1 || event.detail !== 2) return
  if (isInteractiveTarget(event.target, event.currentTarget)) return

  event.preventDefault()
  event.stopPropagation()
  void getCurrentWindow().toggleMaximize()
}
