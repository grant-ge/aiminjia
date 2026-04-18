import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { McpServerForm, parseEnvVars } from './McpServerForm'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

describe('parseEnvVars', () => {
  it('returns undefined for empty strings', () => {
    expect(parseEnvVars('')).toBeUndefined()
    expect(parseEnvVars('   \n  ')).toBeUndefined()
  })

  it('parses newline-delimited env vars', () => {
    expect(parseEnvVars('A=1\nB=two')).toEqual({
      A: '1',
      B: 'two',
    })
  })

  it('keeps equals signs inside env var values', () => {
    expect(parseEnvVars('TOKEN=a=b=c')).toEqual({
      TOKEN: 'a=b=c',
    })
  })
})

describe('McpServerForm', () => {
  it('disables submit when required fields are empty', () => {
    render(
      <McpServerForm
        visible={true}
        onSubmit={vi.fn(async () => {})}
        onCancel={vi.fn()}
        submitting={false}
      />,
    )

    expect(screen.getByRole('button', { name: 'settings.mcp.form.submit' })).toBeDisabled()
  })

  it('disables submit when the name contains spaces', () => {
    render(
      <McpServerForm
        visible={true}
        onSubmit={vi.fn(async () => {})}
        onCancel={vi.fn()}
        submitting={false}
      />,
    )

    const inputs = screen.getAllByRole('textbox')
    fireEvent.change(inputs[0], { target: { value: 'bad name' } })
    fireEvent.change(inputs[1], { target: { value: '/usr/local/bin/demo' } })

    expect(screen.getByRole('button', { name: 'settings.mcp.form.submit' })).toBeDisabled()
    expect(screen.getByText('settings.mcp.form.nameNoSpaces')).toBeInTheDocument()
  })
})
