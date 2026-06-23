import { Button as UiButton, type ButtonProps as UiButtonProps } from '@/components/ui/button'

type CommonButtonVariant = 'primary' | 'secondary' | 'ghost'
type CommonButtonSize = 'sm' | 'md'

interface ButtonProps extends Omit<UiButtonProps, 'variant' | 'size'> {
  variant?: CommonButtonVariant
  size?: CommonButtonSize
}

export function Button({
  variant = 'secondary',
  size = 'md',
  ...props
}: ButtonProps) {
  return <UiButton variant={variant} size={size} {...props} />
}
