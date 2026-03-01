/**
 * SearchSourceBlock — purple-tinted search source references.
 * Based on visual-prototype-zh.html .search-source styles.
 */
import type { SearchSource } from '@/types/message'

interface SearchSourceBlockProps {
  source: SearchSource
}

export function SearchSourceBlock({ source }: SearchSourceBlockProps) {
  return (
    <div
      className="my-3 rounded-lg border p-3.5 text-sm"
      style={{
        background: 'var(--color-semantic-purple-bg-light)',
        borderColor: 'var(--color-semantic-purple-border)',
      }}
    >
      <div
        className="mb-1.5 font-semibold"
        style={{ color: 'var(--color-semantic-purple)' }}
      >
        {source.title}
      </div>

      {source.items.map((item, idx) => (
        <div
          key={idx}
          className="mb-0.5 leading-snug"
          style={{ color: 'var(--color-text-muted)' }}
        >
          <span className="font-medium" style={{ color: 'var(--color-text-secondary)' }}>
            {item.source}
          </span>
          {' — '}
          {item.snippet}
          {item.url && (
            <>
              {' '}
              <a
                href={item.url}
                target="_blank"
                rel="noopener noreferrer"
                className="underline"
                style={{ color: 'var(--color-semantic-purple)' }}
              >
                link
              </a>
            </>
          )}
        </div>
      ))}

      {source.warning && (
        <div
          className="mt-1.5 text-xs italic"
          style={{ color: 'var(--color-semantic-orange)' }}
        >
          {source.warning}
        </div>
      )}
    </div>
  )
}
