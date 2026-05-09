import '@testing-library/jest-dom'
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { MonitoringUrlsForm } from './MonitoringUrlsForm'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

describe('MonitoringUrlsForm', () => {
  it('renders one empty row when initial value is empty', () => {
    render(<MonitoringUrlsForm initial={{}} onSubmit={vi.fn()} onCancel={vi.fn()} />)
    expect(screen.getAllByPlaceholderText('employee.config.monitoringUrls.namePlaceholder')).toHaveLength(1)
  })

  it('preserves existing rows when initial has monitoringTargets', () => {
    render(
      <MonitoringUrlsForm
        initial={{ monitoringTargets: [{ name: 'A', url: 'https://a' }, { name: 'B', url: 'https://b' }] }}
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    expect(screen.getAllByPlaceholderText('employee.config.monitoringUrls.namePlaceholder')).toHaveLength(2)
  })

  it('save button is disabled when any row is empty or invalid', () => {
    render(<MonitoringUrlsForm initial={{}} onSubmit={vi.fn()} onCancel={vi.fn()} />)
    expect(screen.getByRole('button', { name: 'employee.config.save' })).toBeDisabled()
  })

  it('calls onSubmit with normalized monitoringTargets on save', () => {
    const onSubmit = vi.fn()
    render(<MonitoringUrlsForm initial={{}} onSubmit={onSubmit} onCancel={vi.fn()} />)

    fireEvent.change(screen.getAllByPlaceholderText('employee.config.monitoringUrls.namePlaceholder')[0], { target: { value: 'Anthropic' } })
    fireEvent.change(screen.getAllByPlaceholderText('employee.config.monitoringUrls.urlPlaceholder')[0], { target: { value: 'https://anthropic.com' } })

    fireEvent.click(screen.getByRole('button', { name: 'employee.config.save' }))
    expect(onSubmit).toHaveBeenCalledWith({
      monitoringTargets: [{ name: 'Anthropic', url: 'https://anthropic.com', tags: [] }],
    })
  })

  it('allows save with name only when url is left empty', () => {
    const onSubmit = vi.fn()
    render(<MonitoringUrlsForm initial={{}} onSubmit={onSubmit} onCancel={vi.fn()} />)

    fireEvent.change(
      screen.getAllByPlaceholderText('employee.config.monitoringUrls.namePlaceholder')[0],
      { target: { value: 'OpenAI' } },
    )

    const saveBtn = screen.getByRole('button', { name: 'employee.config.save' })
    expect(saveBtn).not.toBeDisabled()

    fireEvent.click(saveBtn)
    expect(onSubmit).toHaveBeenCalledWith({
      monitoringTargets: [{ name: 'OpenAI', url: '', tags: [] }],
    })
  })

  it('rejects save when url is non-empty but malformed', () => {
    render(<MonitoringUrlsForm initial={{}} onSubmit={vi.fn()} onCancel={vi.fn()} />)

    fireEvent.change(
      screen.getAllByPlaceholderText('employee.config.monitoringUrls.namePlaceholder')[0],
      { target: { value: 'X' } },
    )
    fireEvent.change(
      screen.getAllByPlaceholderText('employee.config.monitoringUrls.urlPlaceholder')[0],
      { target: { value: 'not-a-url' } },
    )

    expect(screen.getByRole('button', { name: 'employee.config.save' })).toBeDisabled()
  })
})
