// Vitest setup file — polyfill DOM APIs that jsdom does not implement but
// prosemirror-view depends on for click/coords resolution.
//
// Without these, Tiptap's `useEditor` instance throws "Uncaught Exception"
// noise during user-event clicks even when test assertions still pass.

if (typeof document !== 'undefined') {
  if (typeof document.elementFromPoint !== 'function') {
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      writable: true,
      value: () => null,
    })
  }
}

if (typeof Range !== 'undefined' && !Range.prototype.getClientRects) {
  Range.prototype.getClientRects = function () {
    return [] as unknown as DOMRectList
  }
  Range.prototype.getBoundingClientRect = function () {
    return { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) } as DOMRect
  }
}

if (typeof Element !== 'undefined') {
  if (!Element.prototype.getClientRects) {
    Element.prototype.getClientRects = function () {
      return [] as unknown as DOMRectList
    }
  }
}
