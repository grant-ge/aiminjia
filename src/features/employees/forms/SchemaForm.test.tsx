import { describe, expect, it } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'

// SchemaForm uses i18n; pull the project init so keys resolve. Default
// locale in this repo is zh-CN, so we match button text against the
// Chinese labels ("保存" / "取消").
import '@/i18n'
import { SchemaForm, type JsonSchema } from './SchemaForm'

const SAVE = /保存/
const CANCEL = /取消/

function renderForm(schema: JsonSchema, initial?: Record<string, unknown>) {
  const submitted: Record<string, unknown>[] = []
  const cancelled: { c: number } = { c: 0 }
  const utils = render(
    <SchemaForm
      schema={schema}
      initial={initial}
      onSubmit={(v) => submitted.push(v)}
      onCancel={() => {
        cancelled.c += 1
      }}
    />,
  )
  return { ...utils, submitted, cancelled }
}

describe('SchemaForm', () => {
  it('renders a required string field and submits when filled', async () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: { notes: { type: 'string', title: 'Notes' } },
      required: ['notes'],
    }
    const { submitted } = renderForm(schema)
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'hello' } })
    fireEvent.click(screen.getByRole('button', { name: SAVE }))
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toEqual({ notes: 'hello' })
  })

  it('blocks submit when a required field is empty', async () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: { notes: { type: 'string', title: 'Notes' } },
      required: ['notes'],
    }
    const { submitted } = renderForm(schema)
    fireEvent.click(screen.getByRole('button', { name: SAVE }))
    await new Promise((r) => setTimeout(r, 0))
    expect(submitted).toHaveLength(0)
    expect(screen.getByText(/必填项/)).toBeTruthy()
  })

  it('seeds value from `initial` then from schema `default`, in that order', () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: {
        a: { type: 'string', default: 'fromDefault' },
        b: { type: 'string', default: 'fromDefault' },
      },
    }
    renderForm(schema, { a: 'fromInitial' })
    const inputs = screen.getAllByRole('textbox')
    expect((inputs[0] as HTMLInputElement).value).toBe('fromInitial')
    expect((inputs[1] as HTMLInputElement).value).toBe('fromDefault')
  })

  it('validates URL format', async () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: { url: { type: 'string', title: 'URL', format: 'uri' } },
      required: ['url'],
    }
    const { submitted } = renderForm(schema)
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'not-a-url' } })
    fireEvent.click(screen.getByRole('button', { name: SAVE }))
    await new Promise((r) => setTimeout(r, 0))
    expect(submitted).toHaveLength(0)
    expect(screen.getByText(/合法的 URL/)).toBeTruthy()
  })

  it('coerces numeric string to number on submit', async () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: {
        count: { type: 'integer', title: 'Count', minimum: 1, maximum: 100 },
      },
      required: ['count'],
    }
    const { submitted } = renderForm(schema)
    fireEvent.change(screen.getByRole('spinbutton'), { target: { value: '42' } })
    fireEvent.click(screen.getByRole('button', { name: SAVE }))
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toEqual({ count: 42 })
    expect(typeof submitted[0].count).toBe('number')
  })

  it('renders an enum string as a select and submits the chosen value', async () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: { mode: { type: 'string', title: 'Mode', enum: ['fast', 'slow'] } },
      required: ['mode'],
    }
    const { submitted } = renderForm(schema)
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'slow' } })
    fireEvent.click(screen.getByRole('button', { name: SAVE }))
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toEqual({ mode: 'slow' })
  })

  it('renders array-of-string as tag input and submits collected tags', async () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: {
        tags: { type: 'array', title: 'Tags', items: { type: 'string' } },
      },
    }
    const { submitted } = renderForm(schema)
    const input = screen.getByRole('textbox')
    fireEvent.change(input, { target: { value: 'alpha' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    fireEvent.change(input, { target: { value: 'beta' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    fireEvent.click(screen.getByRole('button', { name: SAVE }))
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toEqual({ tags: ['alpha', 'beta'] })
  })

  it('renders array-of-enum as checkbox group (multi-select)', async () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: {
        dims: {
          type: 'array',
          title: 'Dimensions',
          items: { type: 'string', enum: ['product', 'pricing', 'hiring'] },
        },
      },
    }
    const { submitted } = renderForm(schema, { dims: ['product'] })
    const checkboxes = screen.getAllByRole('checkbox')
    expect(checkboxes).toHaveLength(3)
    expect((checkboxes[0] as HTMLInputElement).checked).toBe(true)
    fireEvent.click(checkboxes[1])
    fireEvent.click(screen.getByRole('button', { name: SAVE }))
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toEqual({ dims: ['product', 'pricing'] })
  })

  it('renders boolean as a checkbox', async () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: { active: { type: 'boolean', title: 'Active' } },
    }
    const { submitted } = renderForm(schema)
    fireEvent.click(screen.getByRole('checkbox'))
    fireEvent.click(screen.getByRole('button', { name: SAVE }))
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toEqual({ active: true })
  })

  it('cancel invokes onCancel', () => {
    const schema: JsonSchema = {
      type: 'object',
      properties: { x: { type: 'string' } },
    }
    const { cancelled } = renderForm(schema)
    fireEvent.click(screen.getByRole('button', { name: CANCEL }))
    expect(cancelled.c).toBe(1)
  })
})
