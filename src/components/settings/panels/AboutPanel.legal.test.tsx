import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/components/ui/button', () => ({
  Button: ({ variant, size, children, ...props }: React.ComponentProps<'button'> & { variant?: string; size?: string }) => (
    <button type="button" data-ui-button="true" data-variant={variant ?? 'default'} data-size={size ?? 'default'} {...props}>
      {children}
    </button>
  ),
}))

import { AboutPanel } from './AboutPanel'

const baseProps = {
  appName: 'AI小家',
  version: '0.9.30-26041603',
  copyright: '仁励家网络科技(杭州)有限公司 版权所有',
  logoUrl: '/brand-avatar-gold.svg',
  onCheckUpdate: vi.fn(),
  onUploadLogs: vi.fn(),
  onResetData: vi.fn(),
  dataMaskingLevel: 'relaxed' as const,
  onDataMaskingChange: vi.fn(),
  appLogLevel: 'info' as const,
  onAppLogLevelChange: vi.fn(),
  links: {
    customerService: vi.fn(),
    productSuggestion: vi.fn(),
    privacyPolicy: vi.fn(),
    terms: vi.fn(),
  },
}

describe('AboutPanel legal documents', () => {
  it('restores local legal document entries through callbacks', () => {
    render(<AboutPanel {...baseProps} />)

    fireEvent.click(screen.getByRole('button', { name: '用户协议' }))
    fireEvent.click(screen.getByRole('button', { name: '隐私政策' }))

    expect(baseProps.links.terms).toHaveBeenCalledTimes(1)
    expect(baseProps.links.privacyPolicy).toHaveBeenCalledTimes(1)
  })
})
