// Polyfills for older WebKit (Safari ≤ 15.3 / macOS Big Sur 11.x).
// Tauri uses the system WebView, so we cannot assume a modern Chrome runtime.
//
// Big Sur Intel ships Safari 14 → missing ES2023 array methods that Tiptap /
// ProseMirror call unconditionally (e.g. `state.tr.steps.findLast(...)` in
// the focus plugin → blank editor + TypeError).
//
// Polyfills must be installed BEFORE any module that uses them is parsed,
// so this file is imported first from main.tsx.

// findLast / findLastIndex (ES2023, Safari 15.4+, Chrome 97+)
// @ts-expect-error — tsc lib target may not include ES2023 yet
if (!Array.prototype.findLast) {
  // eslint-disable-next-line no-extend-native
  Object.defineProperty(Array.prototype, 'findLast', {
    value: function findLast<T>(this: T[], predicate: (v: T, i: number, a: T[]) => unknown): T | undefined {
      for (let i = this.length - 1; i >= 0; i--) {
        if (predicate(this[i], i, this)) return this[i]
      }
      return undefined
    },
    writable: true,
    configurable: true,
  })
}

// @ts-expect-error — see above
if (!Array.prototype.findLastIndex) {
  // eslint-disable-next-line no-extend-native
  Object.defineProperty(Array.prototype, 'findLastIndex', {
    value: function findLastIndex<T>(this: T[], predicate: (v: T, i: number, a: T[]) => unknown): number {
      for (let i = this.length - 1; i >= 0; i--) {
        if (predicate(this[i], i, this)) return i
      }
      return -1
    },
    writable: true,
    configurable: true,
  })
}

// Object.hasOwn (ES2022, Safari 15.4+, Chrome 93+) — used by some deps
if (!Object.hasOwn) {
  Object.defineProperty(Object, 'hasOwn', {
    value: (obj: object, key: PropertyKey): boolean =>
      Object.prototype.hasOwnProperty.call(obj, key),
    writable: true,
    configurable: true,
  })
}

// Array.prototype.at (ES2022, Safari 15.4+)
if (!Array.prototype.at) {
  // eslint-disable-next-line no-extend-native
  Object.defineProperty(Array.prototype, 'at', {
    value: function at<T>(this: T[], n: number): T | undefined {
      const len = this.length
      const k = n < 0 ? len + n : n
      return k >= 0 && k < len ? this[k] : undefined
    },
    writable: true,
    configurable: true,
  })
}

// String.prototype.at (ES2022, Safari 15.4+)
if (!String.prototype.at) {
  // eslint-disable-next-line no-extend-native
  Object.defineProperty(String.prototype, 'at', {
    value: function at(this: string, n: number): string | undefined {
      const len = this.length
      const k = n < 0 ? len + n : n
      return k >= 0 && k < len ? this.charAt(k) : undefined
    },
    writable: true,
    configurable: true,
  })
}

// structuredClone (Safari 15.4+) — graceful fallback via JSON round-trip
// (lossy for Date/Map/Set/RegExp; sufficient for plain data dependencies use)
if (typeof globalThis.structuredClone !== 'function') {
  ;(globalThis as { structuredClone: <T>(v: T) => T }).structuredClone = <T>(v: T): T => {
    if (v === undefined) return undefined as T
    return JSON.parse(JSON.stringify(v))
  }
}
