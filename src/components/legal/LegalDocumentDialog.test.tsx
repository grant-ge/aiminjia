import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'

import { useBrandingStore } from '@/stores/brandingStore'

import { LegalDocumentDialog } from './LegalDocumentDialog'

describe('LegalDocumentDialog', () => {
  beforeEach(() => {
    useBrandingStore.getState().reset()
  })

  it('uses tenant product name in legal dialog title without rewriting document body', () => {
    useBrandingStore.setState({ productName: '小新助手' })

    render(
      <LegalDocumentDialog
        open
        onOpenChange={() => {}}
        document={{
          key: 'terms',
          html: '<p>AI小家软件许可及服务协议</p>',
          titleKey: 'legal.terms.title',
        }}
      />,
    )

    expect(screen.getByRole('dialog', { name: '小新助手软件许可及服务协议' })).toBeInTheDocument()
    expect(screen.getByTitle('小新助手软件许可及服务协议')).toHaveAttribute(
      'srcdoc',
      '<p>AI小家软件许可及服务协议</p>',
    )
  })
})
