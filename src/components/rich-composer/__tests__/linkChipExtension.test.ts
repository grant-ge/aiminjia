import { describe, it, expect } from 'vitest'

import { extractUrls } from '../linkChipExtension'

describe('extractUrls', () => {
  it('finds a single bare URL', () => {
    expect(extractUrls('https://example.com')).toEqual([
      { start: 0, end: 19, url: 'https://example.com' },
    ])
  })

  it('finds URLs in the middle of text', () => {
    const matches = extractUrls('see https://example.com for more')
    expect(matches).toEqual([
      { start: 4, end: 23, url: 'https://example.com' },
    ])
  })

  it('finds multiple URLs in one paste', () => {
    const text = 'https://a.com and https://b.org'
    const matches = extractUrls(text)
    expect(matches.map((m) => m.url)).toEqual(['https://a.com', 'https://b.org'])
  })

  it('trims trailing soft punctuation', () => {
    const matches = extractUrls('go to https://example.com.')
    expect(matches[0].url).toBe('https://example.com')
  })

  it('trims trailing CJK punctuation', () => {
    const matches = extractUrls('看这个 https://example.com。')
    expect(matches[0].url).toBe('https://example.com')
  })

  it('ignores non-http schemes', () => {
    expect(extractUrls('ftp://x.com file://y mailto:a@b.com')).toEqual([])
  })

  it('keeps query strings and fragments', () => {
    const url = 'https://example.com/path?q=1&z=2#frag'
    expect(extractUrls(url)[0].url).toBe(url)
  })

  it('returns no matches for plain text', () => {
    expect(extractUrls('hello world, no link here')).toEqual([])
  })

  it('skips http:// with no host', () => {
    expect(extractUrls('http:// is not a url')).toEqual([])
  })

  it('finds many URLs with no cap', () => {
    const text = Array.from({ length: 20 }, (_, i) => `https://h${i}.com`).join(' ')
    const matches = extractUrls(text)
    expect(matches).toHaveLength(20)
  })
})
