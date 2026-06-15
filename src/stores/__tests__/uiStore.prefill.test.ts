import { beforeEach, describe, expect, it } from 'vitest'

import { useUiStore } from '../uiStore'

describe('uiStore prefillText', () => {
  beforeEach(() => {
    useUiStore.setState({ prefillText: null })
  })

  it('initial prefillText is null', () => {
    expect(useUiStore.getState().prefillText).toBeNull()
  })

  it('setPrefillText stores the value', () => {
    useUiStore.getState().setPrefillText('draft message')
    expect(useUiStore.getState().prefillText).toBe('draft message')
  })

  it('consumePrefillText returns and clears the value', () => {
    useUiStore.getState().setPrefillText('hello')
    const consumed = useUiStore.getState().consumePrefillText()
    expect(consumed).toBe('hello')
    expect(useUiStore.getState().prefillText).toBeNull()
  })

  it('consumePrefillText returns null when empty', () => {
    expect(useUiStore.getState().consumePrefillText()).toBeNull()
  })
})
