import type { HTMLAttributes, ReactNode } from 'react'

import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

export type SegmentedControlSize = 'sm' | 'md' | 'lg'

export const segmentedControlHeightClasses: Record<SegmentedControlSize, string> = {
  sm: 'h-6',
  md: 'h-8',
  lg: 'h-10',
}

const segmentedControlPaddingClasses: Record<SegmentedControlSize, string> = {
  sm: 'p-0.5',
  md: 'p-1',
  lg: 'p-1',
}

const segmentedControlIndicatorInset: Record<SegmentedControlSize, string> = {
  sm: '2px',
  md: '4px',
  lg: '4px',
}

const segmentedControlIndicatorPadding: Record<SegmentedControlSize, string> = {
  sm: '4px',
  md: '8px',
  lg: '8px',
}

const segmentedControlItemClasses: Record<SegmentedControlSize, string> = {
  sm: 'px-2 text-xs',
  md: 'px-3 text-sm',
  lg: 'px-3.5 text-sm',
}

export interface SegmentedControlOption<T extends string> {
  ariaLabel?: string
  disabled?: boolean
  icon?: ReactNode
  label: ReactNode
  title?: string
  tooltip?: ReactNode
  value: T
}

interface SegmentedControlProps<T extends string> extends Omit<HTMLAttributes<HTMLDivElement>, 'onChange'> {
  ariaLabel: string
  className?: string
  disabled?: boolean
  itemClassName?: string
  onValueChange: (value: T) => void
  options: Array<SegmentedControlOption<T>>
  size?: SegmentedControlSize
  testId?: string
  tooltipSide?: 'top' | 'right' | 'bottom' | 'left'
  value: T
}

/**
 * 通用分段选择控件。
 *
 * 使用场景：
 * - 多值互斥选择：字号「小 / 中 / 大」、语言「中文 / English」。
 * - 二值开关：直接传入「关 / 开」两个选项，不再单独维护 Switch 组件。
 * - 图标 Tab：侧边栏这种只显示 icon 的切换，需要给每个 option 提供 ariaLabel。
 *
 * 约定：
 * - 外层是 radiogroup，每个选项是 radio，表示“当前只选中一个值”。
 * - 高度只有 sm / md / lg 三档，和 Button 尺寸保持一致。
 * - 如果是二值开关，value 建议用 off/on，展示文案建议用「关 / 开」。
 */
export function SegmentedControl<T extends string>({
  ariaLabel,
  className,
  disabled = false,
  itemClassName,
  onValueChange,
  options,
  size = 'md',
  testId,
  tooltipSide = 'bottom',
  value,
  ...props
}: SegmentedControlProps<T>) {
  const selectedIndex = Math.max(0, options.findIndex((option) => option.value === value))
  const optionCount = Math.max(1, options.length)

  return (
    <div
      {...props}
      className={cn(
        'relative inline-grid overflow-hidden rounded-md bg-muted text-muted-foreground',
        segmentedControlHeightClasses[size],
        segmentedControlPaddingClasses[size],
        className,
      )}
      data-testid={testId}
      role="radiogroup"
      aria-label={ariaLabel}
      aria-disabled={disabled || undefined}
      style={{
        ...props.style,
        gridTemplateColumns: `repeat(${optionCount}, minmax(0, 1fr))`,
      }}
    >
      <div
        aria-hidden="true"
        data-testid={testId ? `${testId}-indicator` : undefined}
        className="pointer-events-none absolute rounded bg-card shadow-sm transition-transform duration-200 ease-in-out"
        style={{
          top: segmentedControlIndicatorInset[size],
          bottom: segmentedControlIndicatorInset[size],
          left: segmentedControlIndicatorInset[size],
          width: `calc((100% - ${segmentedControlIndicatorPadding[size]}) / ${optionCount})`,
          transform: `translateX(${selectedIndex * 100}%)`,
        }}
      />
      {options.map((option) => {
        const selected = value === option.value
        const optionDisabled = disabled || option.disabled
        const button = (
          <Button
            unstyled
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={option.ariaLabel ?? (typeof option.label === 'string' ? option.label : undefined)}
            title={option.title}
            disabled={optionDisabled}
            onClick={() => {
              if (!optionDisabled) onValueChange(option.value)
            }}
            className={cn(
              'relative z-10 inline-flex min-w-0 items-center justify-center gap-1.5 rounded text-center font-medium transition-colors duration-150',
              segmentedControlItemClasses[size],
              selected
                ? 'text-foreground'
                : 'text-muted-foreground hover:text-foreground',
              optionDisabled && 'cursor-not-allowed opacity-60 hover:text-muted-foreground',
              itemClassName,
            )}
          >
            {option.icon}
            {option.label ? <span className="truncate">{option.label}</span> : null}
          </Button>
        )

        if (!option.tooltip) return button

        return (
          <TooltipProvider key={option.value} delayDuration={400}>
            <Tooltip>
              <TooltipTrigger asChild>{button}</TooltipTrigger>
              <TooltipContent side={tooltipSide}>{option.tooltip}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        )
      })}
    </div>
  )
}
