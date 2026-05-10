/**
 * @designSource design.pen#homeSuggestionPanel
 * @sizing wrapper r-30 border 1 padding [18,24]; row gap 20 padding [20,0]
 */
import type { HomeSuggestionItem } from '@/data/home-suggestions'

interface HomeSuggestionPanelProps {
  items: HomeSuggestionItem[]
  onSelect: (item: HomeSuggestionItem) => void
}

export function HomeSuggestionPanel({
  items,
  onSelect,
}: HomeSuggestionPanelProps) {
  return (
    <div className="w-full px-4 -mt-1.5">
      <div className="flex flex-col">
        {items.map((item, index) => (
          <button
            key={item.key}
            type="button"
            onClick={() => onSelect(item)}
            className={
              index === items.length - 1
                ? 'flex w-full items-start gap-3 py-3 text-left transition-colors hover:text-primary'
                : 'flex w-full items-start gap-3 border-b border-border/80 py-3 text-left transition-colors hover:text-primary'
            }
          >
            <span className="shrink-0 text-sm font-semibold leading-6 text-foreground">
              {item.title}
            </span>
            <span
              aria-hidden="true"
              className="mt-0.5 h-5 w-px shrink-0 bg-border"
            />
            <span className="text-sm leading-6 text-muted-foreground">
              {item.desc}
            </span>
          </button>
        ))}
      </div>
    </div>
  )
}
