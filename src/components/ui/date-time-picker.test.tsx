import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { DateTimePicker } from './date-time-picker'

describe('DateTimePicker', () => {
  it('emits minute-level date time values', () => {
    const onChange = vi.fn()
    render(
      <DateTimePicker
        label="开始时间"
        value="2026-05-07T09:00"
        onChange={onChange}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始时间' }))
    fireEvent.click(screen.getByRole('button', { name: '选择小时 10' }))
    fireEvent.click(screen.getByRole('button', { name: '选择分钟 45' }))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    expect(onChange).toHaveBeenCalledWith('2026-05-07T10:45')
  })

  it('exposes visible date and time controls for intent-test commands', () => {
    render(
      <DateTimePicker
        id="agenda-editor-start"
        label="开始时间"
        value="2026-05-07T09:00"
        onChange={() => {}}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始时间' }))

    expect(screen.getByRole('button', { name: '开始时间' })).toHaveAttribute(
      'data-aijia-date-time-trigger',
      'agenda-editor-start',
    )
    expect(document.querySelector('[data-aijia-calendar-date="2026-05-07"]')).toBeInTheDocument()
    expect(document.querySelector('[data-aijia-time-unit="hour"][data-aijia-time-value="09"]')).toBeInTheDocument()
    expect(document.querySelector('[data-aijia-time-unit="minute"][data-aijia-time-value="00"]')).toBeInTheDocument()
    expect(document.querySelector('[data-aijia-date-time-action="apply"]')).toBeInTheDocument()
  })

  it('keeps the trigger visually flat until focus', () => {
    render(
      <DateTimePicker
        label="开始时间"
        value="2026-05-07T09:00"
        onChange={() => {}}
        level="minute"
      />,
    )

    expect(screen.getByRole('button', { name: '开始时间' })).not.toHaveClass('shadow-sm')
  })

  it('keeps date and time controls in a horizontal layout for small windows', () => {
    render(
      <DateTimePicker
        label="开始时间"
        value="2026-05-07T09:00"
        onChange={() => {}}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始时间' }))

    expect(document.querySelector('[data-aijia-date-time-layout="date-time-horizontal"]')).toBeInTheDocument()
  })

  it('stretches time lists to align with the day calendar in date-time mode', () => {
    render(
      <DateTimePicker
        label="开始时间"
        value="2026-05-07T09:00"
        onChange={() => {}}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始时间' }))

    expect(document.querySelector('[data-aijia-time-panel]')).toHaveClass('h-full')
    document.querySelectorAll('[data-aijia-time-list]').forEach((list) => {
      expect(list).toHaveClass('h-[248px]')
      expect(list).not.toHaveClass('h-[204px]')
      expect(list).not.toHaveClass('flex-1')
    })
  })

  it('offers a today shortcut in the footer', () => {
    const onChange = vi.fn()
    render(
      <DateTimePicker
        label="开始时间"
        value="2026-05-07T09:30"
        onChange={onChange}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始时间' }))
    fireEvent.click(screen.getByRole('button', { name: '今天' }))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    expect(onChange).toHaveBeenCalledWith(expect.stringMatching(/^\d{4}-\d{2}-\d{2}T09:30$/))
  })

  it('keeps time-only lists compact', () => {
    render(
      <DateTimePicker
        mode="time"
        label="触发时间"
        value="09:00"
        onChange={() => {}}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '触发时间' }))

    document.querySelectorAll('[data-aijia-time-list]').forEach((list) => {
      expect(list).toHaveClass('h-[204px]')
      expect(list).not.toHaveClass('flex-1')
    })
  })

  it('scrolls selected time values into view when opened', async () => {
    const scrolledLabels: string[] = []
    const originalScrollIntoView = HTMLElement.prototype.scrollIntoView
    HTMLElement.prototype.scrollIntoView = function scrollIntoViewMock() {
      scrolledLabels.push(this.getAttribute('aria-label') ?? '')
    }

    try {
      render(
        <DateTimePicker
          label="开始时间"
          value="2026-08-26T16:04"
          onChange={() => {}}
          level="minute"
        />,
      )

      fireEvent.click(screen.getByRole('button', { name: '开始时间' }))

      await waitFor(() => {
        expect(scrolledLabels).toEqual(expect.arrayContaining(['选择小时 16', '选择分钟 04']))
      })
    } finally {
      HTMLElement.prototype.scrollIntoView = originalScrollIntoView
    }
  })

  it('keeps the day calendar at six rows when switching months', () => {
    render(
      <DateTimePicker
        label="开始时间"
        value="2026-02-07T09:00"
        onChange={() => {}}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始时间' }))

    expect(document.querySelectorAll('[data-aijia-calendar-day]')).toHaveLength(42)

    fireEvent.click(screen.getByRole('button', { name: '下一页' }))

    expect(document.querySelectorAll('[data-aijia-calendar-day]')).toHaveLength(42)
    expect(document.querySelector('[data-aijia-calendar-day][data-outside-month="true"]')).toBeInTheDocument()
  })

  it('keeps the popover itself from becoming a scroll container', () => {
    render(
      <DateTimePicker
        label="开始时间"
        value="2026-05-07T09:00"
        onChange={() => {}}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始时间' }))

    const popover = document.querySelector('[data-aijia-date-time-popover]')
    expect(popover).toBeInTheDocument()
    expect(popover).not.toHaveClass('overflow-y-auto')
    expect(popover).toHaveClass('overflow-hidden')
  })

  it('renders the popover in a portal so it does not alter parent form layout', () => {
    const { container } = render(
      <DateTimePicker
        label="开始时间"
        value="2026-05-07T09:00"
        onChange={() => {}}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始时间' }))

    expect(container.querySelector('[data-aijia-date-time-popover]')).not.toBeInTheDocument()
    expect(document.body.querySelector('[data-aijia-date-time-popover]')).toBeInTheDocument()
  })

  it('uses visible scroll styling only for time lists', () => {
    render(
      <DateTimePicker
        label="开始时间"
        value="2026-05-07T09:00"
        onChange={() => {}}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始时间' }))

    const timeLists = document.querySelectorAll('[data-aijia-time-list]')
    expect(timeLists).toHaveLength(2)
    timeLists.forEach((list) => {
      expect(list).toHaveClass('overflow-y-auto')
      expect(list.className).toContain('scrollbar-color:var(--muted-foreground)_transparent')
    })
    expect(document.querySelector('[data-aijia-date-panel] [data-aijia-time-list]')).not.toBeInTheDocument()
  })

  it('supports month-level values', () => {
    const onChange = vi.fn()
    render(
      <DateTimePicker
        label="月份"
        value="2026-05"
        onChange={onChange}
        level="month"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '月份' }))
    fireEvent.click(screen.getByRole('button', { name: '选择月份 6' }))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    expect(onChange).toHaveBeenCalledWith('2026-06')
  })

  it('supports time-only minute values', () => {
    const onChange = vi.fn()
    render(
      <DateTimePicker
        mode="time"
        label="触发时间"
        value="09:00"
        onChange={onChange}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '触发时间' }))
    fireEvent.click(screen.getByRole('button', { name: '选择小时 18' }))
    fireEvent.click(screen.getByRole('button', { name: '选择分钟 30' }))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    expect(onChange).toHaveBeenCalledWith('18:30')
  })

  it('supports range values', () => {
    const onChange = vi.fn()
    render(
      <DateTimePicker
        mode="range"
        label="日期范围"
        value={{ start: '2026-05-07T09:00', end: '2026-05-08T18:00' }}
        onChange={onChange}
        level="minute"
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '日期范围' }))
    fireEvent.click(screen.getByRole('button', { name: '结束' }))
    fireEvent.click(screen.getByRole('button', { name: '选择小时 19' }))
    fireEvent.click(screen.getByRole('button', { name: '选择分钟 15' }))
    fireEvent.click(screen.getByRole('button', { name: '确定' }))

    expect(onChange).toHaveBeenCalledWith({
      start: '2026-05-07T09:00',
      end: '2026-05-08T19:15',
    })
  })
})
