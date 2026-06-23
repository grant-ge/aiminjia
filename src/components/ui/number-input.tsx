import * as React from 'react'

import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'

type NumberInputProps = Omit<
  React.ComponentProps<typeof Input>,
  'type' | 'value' | 'defaultValue' | 'onChange'
> & {
  value: number
  onValueChange: (value: number) => void
}

const NumberInput = React.forwardRef<HTMLInputElement, NumberInputProps>(
  ({ value, onValueChange, min, max, step = 1, className, ...props }, ref) => {
    const clamp = (next: number) => {
      let clamped = next
      if (typeof min === 'number') clamped = Math.max(min, clamped)
      if (typeof max === 'number') clamped = Math.min(max, clamped)
      return clamped
    }

    return (
      <Input
        ref={ref}
        type="number"
        inputMode="numeric"
        min={min}
        max={max}
        step={step}
        value={value}
        className={cn(
          '[appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none',
          className,
        )}
        onChange={(event) => {
          const parsed = Number(event.target.value)
          onValueChange(clamp(Number.isFinite(parsed) ? parsed : Number(min ?? 0)))
        }}
        {...props}
      />
    )
  },
)
NumberInput.displayName = 'NumberInput'

export { NumberInput }
