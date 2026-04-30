/** Shared color utilities for runtime theming. */

export function hexToRgb(hex: string): [number, number, number] {
  // Normalize: support #RGB shorthand
  const clean = hex.startsWith('#') ? hex.slice(1) : hex
  const full = clean.length === 3
    ? clean.split('').map((c) => c + c).join('')
    : clean
  if (full.length !== 6 || !/^[0-9A-Fa-f]{6}$/.test(full)) {
    return [0, 0, 0] // fallback to black on invalid input
  }
  const r = parseInt(full.slice(0, 2), 16)
  const g = parseInt(full.slice(2, 4), 16)
  const b = parseInt(full.slice(4, 6), 16)
  return [r, g, b]
}

export function rgbToHex(r: number, g: number, b: number): string {
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)))
  return `#${clamp(r).toString(16).padStart(2, '0')}${clamp(g).toString(16).padStart(2, '0')}${clamp(b).toString(16).padStart(2, '0')}`
}

export function lighten(hex: string, factor: number): string {
  const [r, g, b] = hexToRgb(hex)
  return rgbToHex(r + (255 - r) * factor, g + (255 - g) * factor, b + (255 - b) * factor)
}

export function darken(hex: string, factor: number): string {
  const [r, g, b] = hexToRgb(hex)
  return rgbToHex(r * (1 - factor), g * (1 - factor), b * (1 - factor))
}

export function rgba(hex: string, alpha: number): string {
  const [r, g, b] = hexToRgb(hex)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

export function isDarkColor(hex: string): boolean {
  const [r, g, b] = hexToRgb(hex)
  return (r * 0.299 + g * 0.587 + b * 0.114) < 128
}

export function mix(hex: string, other: string, weightOfOther: number): string {
  const [r1, g1, b1] = hexToRgb(hex)
  const [r2, g2, b2] = hexToRgb(other)
  const weightOfBase = 1 - weightOfOther

  return rgbToHex(
    r1 * weightOfBase + r2 * weightOfOther,
    g1 * weightOfBase + g2 * weightOfOther,
    b1 * weightOfBase + b2 * weightOfOther,
  )
}
