import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { ConfirmDialogHost, requestConfirm, useConfirmDialogStore } from './ConfirmDialogHost'

afterEach(() => {
  useConfirmDialogStore.setState({ request: null })
})

describe('ConfirmDialogHost', () => {
  it('renders requested confirmations in the shared dialog and resolves true on confirm', async () => {
    render(<ConfirmDialogHost />)

    let result!: Promise<boolean>
    act(() => {
      result = requestConfirm({
        title: '删除 MCP 服务器？',
        description: '确认删除 MCP 服务器「demo」？',
        confirmLabel: '确认删除',
        variant: 'destructive',
      })
    })

    expect(await screen.findByText('删除 MCP 服务器？')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '确认删除' })).toHaveClass('bg-destructive')

    fireEvent.click(screen.getByRole('button', { name: '确认删除' }))

    await expect(result).resolves.toBe(true)
  })

  it('resolves false when cancelled', async () => {
    render(<ConfirmDialogHost />)

    let result!: Promise<boolean>
    act(() => {
      result = requestConfirm({
        title: '卸载技能？',
        description: '确定要卸载技能「demo」吗？',
        confirmLabel: '卸载',
        variant: 'destructive',
      })
    })

    expect(await screen.findByText('卸载技能？')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '取消' }))

    await waitFor(() => expect(screen.queryByText('卸载技能？')).not.toBeInTheDocument())
    await expect(result).resolves.toBe(false)
  })
})
