import * as React from 'react'
import { Calendar, Check, ChevronLeft, ChevronRight, Clock } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { cn } from '@/lib/utils'

export type DateTimePickerLevel = 'year' | 'month' | 'day' | 'hour' | 'minute' | 'second'
export type DateTimePickerMode = 'single' | 'range' | 'time'

export interface DateTimeRangeValue {
  start: string
  end: string
}

type DateTimePickerProps =
  | {
      mode?: 'single'
      value: string
      onChange: (value: string) => void
      level?: DateTimePickerLevel
      label: string
      placeholder?: string
      disabled?: boolean
      className?: string
      id?: string
      locale?: string
    }
  | {
      mode: 'range'
      value: DateTimeRangeValue
      onChange: (value: DateTimeRangeValue) => void
      level?: DateTimePickerLevel
      label: string
      placeholder?: string
      disabled?: boolean
      className?: string
      id?: string
      locale?: string
    }
  | {
      mode: 'time'
      value: string
      onChange: (value: string) => void
      level?: Extract<DateTimePickerLevel, 'hour' | 'minute' | 'second'>
      label: string
      placeholder?: string
      disabled?: boolean
      className?: string
      id?: string
      locale?: string
    }

interface DateParts {
  year: number
  month: number
  day: number
  hour: number
  minute: number
  second: number
}

const LEVEL_RANK: Record<DateTimePickerLevel, number> = {
  year: 0,
  month: 1,
  day: 2,
  hour: 3,
  minute: 4,
  second: 5,
}

const WEEKDAYS_ZH = ['一', '二', '三', '四', '五', '六', '日']
const MONTHS_ZH = ['1月', '2月', '3月', '4月', '5月', '6月', '7月', '8月', '9月', '10月', '11月', '12月']
const DAY_GRID_SIZE = 42

export function DateTimePicker(props: DateTimePickerProps) {
  if (props.mode === 'range') return <RangeDateTimePicker {...props} />
  if (props.mode === 'time') return <SingleDateTimePicker {...props} />
  return <SingleDateTimePicker {...props} mode="single" />
}

function SingleDateTimePicker(
  props: Extract<DateTimePickerProps, { mode?: 'single' }> | Extract<DateTimePickerProps, { mode: 'time' }>,
) {
  const { value, onChange, label, placeholder, disabled, className, id, locale } = props
  const mode = props.mode ?? 'single'
  const level = props.level ?? (mode === 'time' ? 'minute' : 'minute')
  const [open, setOpen] = React.useState(false)
  const [draft, setDraft] = React.useState<DateParts>(() => parseValue(value, level, mode === 'time'))

  React.useEffect(() => {
    if (open) setDraft(parseValue(value, level, mode === 'time'))
  }, [level, mode, open, value])

  const display = formatDisplay(value, level, mode === 'time', locale)

  const apply = () => {
    onChange(formatParts(draft, level, mode === 'time'))
    setOpen(false)
  }

  const selectToday = () => {
    setDraft((prev) => mergeToday(prev, level))
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          id={id}
          type="button"
          variant="outline"
          aria-label={label}
          data-aijia-date-time-trigger={id ?? label}
          disabled={disabled}
          className={cn(
            'h-9 w-full justify-between border-input bg-card px-3 text-left font-normal hover:border-primary hover:bg-card focus-visible:border-primary focus-visible:ring-primary/15',
            !display && 'text-muted-foreground',
            className,
          )}
        >
          <span className="min-w-0 truncate">{display || placeholder || label}</span>
          {mode === 'time' ? <Clock className="h-4 w-4 text-muted-foreground" /> : <Calendar className="h-4 w-4 text-muted-foreground" />}
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        data-aijia-date-time-popover
        className={cn('max-h-[calc(100vh-24px)] overflow-hidden p-3', popoverWidthClass(mode, level))}
      >
        <PickerContent
          draft={draft}
          level={level}
          timeOnly={mode === 'time'}
          locale={locale}
          onDraftChange={setDraft}
        />
        <div className="mt-3 flex items-center justify-between border-t border-border pt-3">
          {mode === 'time' ? (
            <span />
          ) : (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              data-aijia-date-time-action="today"
              onClick={selectToday}
            >
              {isZh(locale) ? '今天' : 'Today'}
            </Button>
          )}
          <Button type="button" size="sm" data-aijia-date-time-action="apply" onClick={apply}>
            <Check className="h-4 w-4" />
            {isZh(locale) ? '确定' : 'Done'}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}

function RangeDateTimePicker(props: Extract<DateTimePickerProps, { mode: 'range' }>) {
  const { value, onChange, label, placeholder, disabled, className, id, level = 'minute', locale } = props
  const [open, setOpen] = React.useState(false)
  const [active, setActive] = React.useState<'start' | 'end'>('start')
  const [draft, setDraft] = React.useState({
    start: parseValue(value.start, level, false),
    end: parseValue(value.end, level, false),
  })

  React.useEffect(() => {
    if (open) {
      setDraft({
        start: parseValue(value.start, level, false),
        end: parseValue(value.end, level, false),
      })
    }
  }, [level, open, value.end, value.start])

  const startDisplay = formatDisplay(value.start, level, false, locale)
  const endDisplay = formatDisplay(value.end, level, false, locale)
  const display = startDisplay && endDisplay ? `${startDisplay} - ${endDisplay}` : ''

  const apply = () => {
    onChange({
      start: formatParts(draft.start, level, false),
      end: formatParts(draft.end, level, false),
    })
    setOpen(false)
  }

  const selectToday = () => {
    setDraft((prev) => ({ ...prev, [active]: mergeToday(prev[active], level) }))
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          id={id}
          type="button"
          variant="outline"
          aria-label={label}
          disabled={disabled}
          className={cn(
            'h-9 w-full justify-between border-input bg-card px-3 text-left font-normal hover:border-primary hover:bg-card focus-visible:border-primary focus-visible:ring-primary/15',
            !display && 'text-muted-foreground',
            className,
          )}
        >
          <span className="min-w-0 truncate">{display || placeholder || label}</span>
          <Calendar className="h-4 w-4 text-muted-foreground" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        data-aijia-date-time-popover
        className={cn('max-h-[calc(100vh-24px)] overflow-hidden p-3', popoverWidthClass('single', level))}
      >
        <div className="mb-3 grid grid-cols-2 gap-2">
          {(['start', 'end'] as const).map((side) => (
            <Button
              key={side}
              type="button"
              size="sm"
              variant={active === side ? 'secondary' : 'outline'}
              onClick={() => setActive(side)}
            >
              {side === 'start' ? (isZh(locale) ? '开始' : 'Start') : isZh(locale) ? '结束' : 'End'}
            </Button>
          ))}
        </div>
        <PickerContent
          draft={draft[active]}
          level={level}
          timeOnly={false}
          locale={locale}
          onDraftChange={(next) => setDraft((prev) => ({ ...prev, [active]: next }))}
        />
        <div className="mt-3 flex items-center justify-between border-t border-border pt-3">
          <Button
            type="button"
            size="sm"
            variant="ghost"
            data-aijia-date-time-action="today"
            onClick={selectToday}
          >
            {isZh(locale) ? '今天' : 'Today'}
          </Button>
          <Button type="button" size="sm" data-aijia-date-time-action="apply" onClick={apply}>
            <Check className="h-4 w-4" />
            {isZh(locale) ? '确定' : 'Done'}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}

interface PickerContentProps {
  draft: DateParts
  level: DateTimePickerLevel
  timeOnly: boolean
  locale?: string
  onDraftChange: (next: DateParts) => void
}

function PickerContent({ draft, level, timeOnly, locale, onDraftChange }: PickerContentProps) {
  const needsDate = !timeOnly
  const needsTime = timeOnly || LEVEL_RANK[level] >= LEVEL_RANK.hour

  return (
    <div
      data-aijia-date-time-layout={needsDate && needsTime ? 'date-time-horizontal' : timeOnly ? 'time-only' : 'date-only'}
      className={cn(
        'grid gap-3',
        needsDate && needsTime ? 'grid-cols-[minmax(0,208px)_minmax(0,1fr)]' : 'grid-cols-1',
      )}
    >
      {needsDate ? (
        <DatePanel draft={draft} level={level} locale={locale} onDraftChange={onDraftChange} />
      ) : null}
      {needsTime ? (
        <TimePanel
          draft={draft}
          level={level}
          locale={locale}
          stretch={needsDate}
          onDraftChange={onDraftChange}
        />
      ) : null}
    </div>
  )
}

function DatePanel({ draft, level, locale, onDraftChange }: Omit<PickerContentProps, 'timeOnly'>) {
  if (level === 'year') {
    const decadeStart = Math.floor(draft.year / 12) * 12
    return (
      <div data-aijia-date-panel>
        <PickerHeader
          title={`${decadeStart} - ${decadeStart + 11}`}
          onPrev={() => onDraftChange({ ...draft, year: draft.year - 12 })}
          onNext={() => onDraftChange({ ...draft, year: draft.year + 12 })}
        />
        <div className="grid grid-cols-3 gap-1">
          {Array.from({ length: 12 }, (_, index) => decadeStart + index).map((year) => (
            <PickerCell
              key={year}
              selected={draft.year === year}
              ariaLabel={isZh(locale) ? `选择年份 ${year}` : `Select year ${year}`}
              onClick={() => onDraftChange({ ...draft, year })}
            >
              {year}
            </PickerCell>
          ))}
        </div>
      </div>
    )
  }

  if (level === 'month') {
    return (
      <div data-aijia-date-panel>
        <PickerHeader
          title={`${draft.year}`}
          onPrev={() => onDraftChange({ ...draft, year: draft.year - 1 })}
          onNext={() => onDraftChange({ ...draft, year: draft.year + 1 })}
        />
        <div className="grid grid-cols-3 gap-1">
          {MONTHS_ZH.map((label, index) => {
            const month = index + 1
            return (
              <PickerCell
                key={month}
                selected={draft.month === month}
                ariaLabel={isZh(locale) ? `选择月份 ${month}` : `Select month ${month}`}
                onClick={() => onDraftChange(clampDay({ ...draft, month }))}
              >
                {isZh(locale) ? label : new Date(2026, index, 1).toLocaleString('en-US', { month: 'short' })}
              </PickerCell>
            )
          })}
        </div>
      </div>
    )
  }

  const cells = buildDayCells(draft.year, draft.month)

  return (
    <div data-aijia-date-panel>
      <PickerHeader
        title={isZh(locale) ? `${draft.year}年${draft.month}月` : `${draft.year}-${pad(draft.month)}`}
        onPrev={() => onDraftChange(addMonths(draft, -1))}
        onNext={() => onDraftChange(addMonths(draft, 1))}
      />
      <div className="mb-1 grid grid-cols-7 gap-1 text-center text-[11px] text-muted-foreground">
        {WEEKDAYS_ZH.map((day, index) => (
          <div key={day}>{isZh(locale) ? day : ['M', 'T', 'W', 'T', 'F', 'S', 'S'][index]}</div>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-1">
        {cells.map((cell) => (
          <PickerCell
            key={`${cell.year}-${cell.month}-${cell.day}`}
            selected={!cell.outsideMonth && draft.day === cell.day}
            muted={cell.outsideMonth}
            ariaLabel={isZh(locale) ? `选择 ${cell.year}-${pad(cell.month)}-${pad(cell.day)}` : `Select ${cell.year}-${pad(cell.month)}-${pad(cell.day)}`}
            onClick={() => onDraftChange({
              ...draft,
              year: cell.year,
              month: cell.month,
              day: cell.day,
            })}
            square
            data-aijia-calendar-day
            data-aijia-calendar-date={`${cell.year}-${pad(cell.month)}-${pad(cell.day)}`}
            data-outside-month={cell.outsideMonth ? 'true' : undefined}
          >
            {cell.day}
          </PickerCell>
        ))}
      </div>
    </div>
  )
}

function PickerHeader({ title, onPrev, onNext }: { title: string; onPrev: () => void; onNext: () => void }) {
  return (
    <div className="mb-2 flex items-center justify-between">
      <Button type="button" variant="ghost" size="icon" className="h-7 w-7" onClick={onPrev} aria-label="上一页">
        <ChevronLeft className="h-4 w-4" />
      </Button>
      <div className="text-sm font-medium text-foreground">{title}</div>
      <Button type="button" variant="ghost" size="icon" className="h-7 w-7" onClick={onNext} aria-label="下一页">
        <ChevronRight className="h-4 w-4" />
      </Button>
    </div>
  )
}

function TimePanel({
  draft,
  level,
  locale,
  stretch,
  onDraftChange,
}: Omit<PickerContentProps, 'timeOnly'> & { stretch: boolean }) {
  const minuteVisible = LEVEL_RANK[level] >= LEVEL_RANK.minute
  const secondVisible = LEVEL_RANK[level] >= LEVEL_RANK.second
  return (
    <div
      data-aijia-time-panel
      className={cn(
        'grid gap-2 border-l border-border pl-3',
        stretch && 'h-full min-h-0',
        secondVisible ? 'grid-cols-3' : minuteVisible ? 'grid-cols-2' : 'grid-cols-1',
      )}
    >
      <TimeColumn
        unit="hour"
        label={isZh(locale) ? '时' : 'Hour'}
        values={range(24)}
        selected={draft.hour}
        ariaPrefix={isZh(locale) ? '选择小时' : 'Select hour'}
        stretch={stretch}
        onSelect={(hour) => onDraftChange({ ...draft, hour })}
      />
      {minuteVisible ? (
        <TimeColumn
          unit="minute"
          label={isZh(locale) ? '分' : 'Minute'}
          values={range(60)}
          selected={draft.minute}
          ariaPrefix={isZh(locale) ? '选择分钟' : 'Select minute'}
          stretch={stretch}
          onSelect={(minute) => onDraftChange({ ...draft, minute })}
        />
      ) : null}
      {secondVisible ? (
        <TimeColumn
          unit="second"
          label={isZh(locale) ? '秒' : 'Second'}
          values={range(60)}
          selected={draft.second}
          ariaPrefix={isZh(locale) ? '选择秒钟' : 'Select second'}
          stretch={stretch}
          onSelect={(second) => onDraftChange({ ...draft, second })}
        />
      ) : null}
    </div>
  )
}

function TimeColumn({
  unit,
  label,
  values,
  selected,
  ariaPrefix,
  stretch,
  onSelect,
}: {
  unit: 'hour' | 'minute' | 'second'
  label: string
  values: number[]
  selected: number
  ariaPrefix: string
  stretch: boolean
  onSelect: (value: number) => void
}) {
  const selectedRef = React.useRef<HTMLButtonElement | null>(null)

  React.useEffect(() => {
    selectedRef.current?.scrollIntoView?.({ block: 'center' })
  }, [selected])

  return (
    <div>
      <div className="mb-1 text-center text-[11px] text-muted-foreground">{label}</div>
      <div
        data-aijia-time-list
        data-aijia-time-unit={unit}
        className={cn(
          'overflow-y-auto rounded-md border border-border p-1 pr-1.5 [scrollbar-color:var(--muted-foreground)_transparent] [scrollbar-width:thin] [&::-webkit-scrollbar]:w-2 [&::-webkit-scrollbar-thumb]:rounded-md [&::-webkit-scrollbar-thumb]:bg-muted-foreground/45 [&::-webkit-scrollbar-track]:bg-transparent',
          stretch ? 'h-[248px]' : 'h-[204px]',
        )}
      >
        {values.map((value) => (
          <button
            key={value}
            ref={selected === value ? selectedRef : undefined}
            type="button"
            aria-label={`${ariaPrefix} ${pad(value)}`}
            data-aijia-time-unit={unit}
            data-aijia-time-value={pad(value)}
            className={cn(
              'mb-1 flex h-7 w-full items-center justify-center rounded-md text-xs text-foreground transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
              selected === value && 'bg-primary text-primary-foreground hover:bg-primary',
            )}
            onClick={() => onSelect(value)}
          >
            {pad(value)}
          </button>
        ))}
      </div>
    </div>
  )
}

function PickerCell({
  children,
  selected,
  muted = false,
  ariaLabel,
  onClick,
  square = false,
  ...props
}: {
  children: React.ReactNode
  selected: boolean
  muted?: boolean
  ariaLabel: string
  onClick: () => void
  square?: boolean
} & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      type="button"
      aria-label={ariaLabel}
      className={cn(
        'rounded-md text-sm text-foreground transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        square ? 'h-8' : 'h-9 px-2',
        muted && 'text-muted-foreground/55',
        selected && 'bg-primary text-primary-foreground hover:bg-primary',
      )}
      onClick={onClick}
      {...props}
    >
      {children}
    </button>
  )
}

interface DayCell {
  year: number
  month: number
  day: number
  outsideMonth: boolean
}

function buildDayCells(year: number, month: number): DayCell[] {
  const first = new Date(year, month - 1, 1)
  const firstWeekday = (first.getDay() + 6) % 7
  const gridStart = new Date(year, month - 1, 1 - firstWeekday)

  return Array.from({ length: DAY_GRID_SIZE }, (_, index) => {
    const date = new Date(gridStart)
    date.setDate(gridStart.getDate() + index)
    const cellYear = date.getFullYear()
    const cellMonth = date.getMonth() + 1
    return {
      year: cellYear,
      month: cellMonth,
      day: date.getDate(),
      outsideMonth: cellYear !== year || cellMonth !== month,
    }
  })
}

function parseValue(value: string, level: DateTimePickerLevel, timeOnly: boolean): DateParts {
  const now = new Date()
  const fallback: DateParts = {
    year: now.getFullYear(),
    month: now.getMonth() + 1,
    day: now.getDate(),
    hour: 9,
    minute: 0,
    second: 0,
  }
  if (!value) return fallback

  if (timeOnly) {
    const match = value.match(/^(\d{1,2})(?::(\d{1,2}))?(?::(\d{1,2}))?$/)
    if (!match) return fallback
    return {
      ...fallback,
      hour: clamp(Number(match[1]), 0, 23),
      minute: clamp(Number(match[2] ?? 0), 0, 59),
      second: clamp(Number(match[3] ?? 0), 0, 59),
    }
  }

  const [datePart, timePart] = value.split('T')
  const [year, month = 1, day = 1] = datePart.split('-').map(Number)
  if (!year || Number.isNaN(year)) return fallback
  const [hour = 9, minute = 0, second = 0] = (timePart ?? '').split(':').map(Number)
  return clampDay({
    year,
    month: clamp(month, 1, 12),
    day: clamp(day, 1, 31),
    hour: clamp(hour, 0, 23),
    minute: LEVEL_RANK[level] >= LEVEL_RANK.minute ? clamp(minute, 0, 59) : 0,
    second: LEVEL_RANK[level] >= LEVEL_RANK.second ? clamp(second, 0, 59) : 0,
  })
}

function formatParts(parts: DateParts, level: DateTimePickerLevel, timeOnly: boolean): string {
  if (timeOnly) {
    if (level === 'hour') return pad(parts.hour)
    if (level === 'second') return `${pad(parts.hour)}:${pad(parts.minute)}:${pad(parts.second)}`
    return `${pad(parts.hour)}:${pad(parts.minute)}`
  }
  const date = `${parts.year}-${pad(parts.month)}-${pad(parts.day)}`
  if (level === 'year') return String(parts.year)
  if (level === 'month') return `${parts.year}-${pad(parts.month)}`
  if (level === 'day') return date
  if (level === 'hour') return `${date}T${pad(parts.hour)}`
  if (level === 'second') return `${date}T${pad(parts.hour)}:${pad(parts.minute)}:${pad(parts.second)}`
  return `${date}T${pad(parts.hour)}:${pad(parts.minute)}`
}

function formatDisplay(value: string, level: DateTimePickerLevel, timeOnly: boolean, locale?: string): string {
  if (!value) return ''
  const parts = parseValue(value, level, timeOnly)
  if (timeOnly) return formatParts(parts, level, true)
  if (isZh(locale)) {
    if (level === 'year') return `${parts.year}年`
    if (level === 'month') return `${parts.year}年${parts.month}月`
    if (level === 'day') return `${parts.year}年${parts.month}月${parts.day}日`
    if (level === 'hour') return `${parts.year}年${parts.month}月${parts.day}日 ${pad(parts.hour)}时`
    if (level === 'second') return `${parts.year}年${parts.month}月${parts.day}日 ${pad(parts.hour)}:${pad(parts.minute)}:${pad(parts.second)}`
    return `${parts.year}年${parts.month}月${parts.day}日 ${pad(parts.hour)}:${pad(parts.minute)}`
  }
  return formatParts(parts, level, false).replace('T', ' ')
}

function addMonths(parts: DateParts, delta: number): DateParts {
  const date = new Date(parts.year, parts.month - 1 + delta, 1)
  return clampDay({
    ...parts,
    year: date.getFullYear(),
    month: date.getMonth() + 1,
  })
}

function clampDay(parts: DateParts): DateParts {
  return {
    ...parts,
    day: clamp(parts.day, 1, daysInMonth(parts.year, parts.month)),
  }
}

function mergeToday(parts: DateParts, level: DateTimePickerLevel): DateParts {
  const today = new Date()
  return clampDay({
    ...parts,
    year: today.getFullYear(),
    month: today.getMonth() + 1,
    day: today.getDate(),
    hour: LEVEL_RANK[level] >= LEVEL_RANK.hour ? parts.hour : 0,
    minute: LEVEL_RANK[level] >= LEVEL_RANK.minute ? parts.minute : 0,
    second: LEVEL_RANK[level] >= LEVEL_RANK.second ? parts.second : 0,
  })
}

function daysInMonth(year: number, month: number): number {
  return new Date(year, month, 0).getDate()
}

function range(count: number): number[] {
  return Array.from({ length: count }, (_, index) => index)
}

function clamp(value: number, min: number, max: number): number {
  if (Number.isNaN(value)) return min
  return Math.min(max, Math.max(min, value))
}

function pad(value: number): string {
  return String(value).padStart(2, '0')
}

function isZh(locale?: string): boolean {
  return !locale || locale.toLowerCase().startsWith('zh')
}

function popoverWidthClass(mode: 'single' | 'time', level: DateTimePickerLevel): string {
  if (mode === 'time') {
    return LEVEL_RANK[level] >= LEVEL_RANK.second ? 'w-[276px] max-w-[calc(100vw-24px)]' : 'w-[204px] max-w-[calc(100vw-24px)]'
  }
  if (LEVEL_RANK[level] >= LEVEL_RANK.second) return 'w-[476px] max-w-[calc(100vw-24px)]'
  if (LEVEL_RANK[level] >= LEVEL_RANK.hour) return 'w-[420px] max-w-[calc(100vw-24px)]'
  return 'w-[300px] max-w-[calc(100vw-24px)]'
}
