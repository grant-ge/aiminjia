import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { Input } from './input'
import { Textarea } from './textarea'
import { FormField } from './field'

describe('FormField', () => {
  it('renders a label, description, and error around an input control', () => {
    render(
      <FormField
        htmlFor="schedule-title"
        label="标题"
        description="给这条日程一个短名称"
        error="标题不能为空"
      >
        <Input id="schedule-title" aria-invalid />
      </FormField>,
    )

    expect(screen.getByLabelText('标题')).toBeInTheDocument()
    expect(screen.getByText('给这条日程一个短名称')).toHaveClass('text-muted-foreground')
    expect(screen.getByText('标题不能为空')).toHaveClass('text-destructive')
  })

  it('keeps textarea fields on the same form rhythm', () => {
    render(
      <FormField htmlFor="schedule-prompt" label="到点要做什么？">
        <Textarea id="schedule-prompt" />
      </FormField>,
    )

    expect(screen.getByLabelText('到点要做什么？')).toBeInstanceOf(HTMLTextAreaElement)
  })

  it('uses a polished form label rhythm instead of tiny placeholder-only fields', () => {
    render(
      <FormField htmlFor="schedule-title" label="标题">
        <Input id="schedule-title" />
      </FormField>,
    )

    const label = screen.getByText('标题')
    expect(label).toHaveClass('text-sm')
    expect(label).toHaveClass('text-foreground')
    expect(label).not.toHaveClass('text-xs')
  })

  it('uses the global primary token for focus emphasis', () => {
    render(<Input aria-label="标题" />)

    const input = screen.getByLabelText('标题')
    expect(input).toHaveClass('hover:border-primary')
    expect(input).toHaveClass('focus-visible:border-primary')
    expect(input.className).not.toContain('#1677ff')
  })

  it('keeps inputs visually flat until focus', () => {
    render(
      <FormField htmlFor="schedule-title" label="标题">
        <Input id="schedule-title" />
      </FormField>,
    )

    expect(screen.getByLabelText('标题')).not.toHaveClass('shadow-sm')
  })

  it('keeps textarea visually flat until focus', () => {
    render(
      <FormField htmlFor="schedule-prompt" label="到点要做什么？">
        <Textarea id="schedule-prompt" />
      </FormField>,
    )

    expect(screen.getByLabelText('到点要做什么？')).not.toHaveClass('shadow-sm')
  })
})
