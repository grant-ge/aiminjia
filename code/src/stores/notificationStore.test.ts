import { describe, it, expect, beforeEach } from 'vitest'
import { useNotificationStore } from './notificationStore'

// Reset store between tests
beforeEach(() => {
  useNotificationStore.setState({ notifications: [] })
})

// ---------------------------------------------------------------------------
// Push notifications
// ---------------------------------------------------------------------------

describe('notificationStore — push', () => {
  it('starts with empty notifications', () => {
    expect(useNotificationStore.getState().notifications).toEqual([])
  })

  it('pushes a notification with auto-generated id', () => {
    useNotificationStore.getState().push({
      level: 'info',
      title: 'Test',
      message: 'Hello',
      actions: [],
      dismissible: true,
      context: 'toast',
    })

    const notifications = useNotificationStore.getState().notifications
    expect(notifications).toHaveLength(1)
    expect(notifications[0].id).toMatch(/^notif_/)
    expect(notifications[0].title).toBe('Test')
    expect(notifications[0].level).toBe('info')
    expect(notifications[0].createdAt).toBeGreaterThan(0)
  })

  it('pushes multiple notifications', () => {
    const { push } = useNotificationStore.getState()

    push({
      level: 'info',
      title: 'First',
      message: 'msg1',
      actions: [],
      dismissible: true,
      context: 'toast',
    })
    push({
      level: 'error',
      title: 'Second',
      message: 'msg2',
      actions: [],
      dismissible: false,
      context: 'banner',
    })

    const notifications = useNotificationStore.getState().notifications
    expect(notifications).toHaveLength(2)
    expect(notifications[0].title).toBe('First')
    expect(notifications[1].title).toBe('Second')
    // IDs should be unique
    expect(notifications[0].id).not.toBe(notifications[1].id)
  })

  it('supports notification actions', () => {
    let clicked = false
    useNotificationStore.getState().push({
      level: 'warning',
      title: 'Action Test',
      message: 'Click me',
      actions: [
        { label: 'Retry', action: () => { clicked = true }, primary: true },
      ],
      dismissible: true,
      context: 'toast',
    })

    const notification = useNotificationStore.getState().notifications[0]
    expect(notification.actions).toHaveLength(1)
    expect(notification.actions[0].label).toBe('Retry')
    notification.actions[0].action()
    expect(clicked).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// Dismiss
// ---------------------------------------------------------------------------

describe('notificationStore — dismiss', () => {
  it('dismisses a notification by id', () => {
    useNotificationStore.getState().push({
      level: 'info',
      title: 'To dismiss',
      message: 'msg',
      actions: [],
      dismissible: true,
      context: 'toast',
    })
    useNotificationStore.getState().push({
      level: 'error',
      title: 'To keep',
      message: 'msg',
      actions: [],
      dismissible: true,
      context: 'banner',
    })

    const idToDismiss = useNotificationStore.getState().notifications[0].id
    useNotificationStore.getState().dismiss(idToDismiss)

    const remaining = useNotificationStore.getState().notifications
    expect(remaining).toHaveLength(1)
    expect(remaining[0].title).toBe('To keep')
  })

  it('dismiss with non-existent id does nothing', () => {
    useNotificationStore.getState().push({
      level: 'info',
      title: 'Test',
      message: 'msg',
      actions: [],
      dismissible: true,
      context: 'toast',
    })
    useNotificationStore.getState().dismiss('nonexistent')
    expect(useNotificationStore.getState().notifications).toHaveLength(1)
  })
})

// ---------------------------------------------------------------------------
// Dismiss all
// ---------------------------------------------------------------------------

describe('notificationStore — dismissAll', () => {
  it('clears all notifications', () => {
    const { push, dismissAll } = useNotificationStore.getState()

    push({ level: 'info', title: 'A', message: '', actions: [], dismissible: true, context: 'toast' })
    push({ level: 'error', title: 'B', message: '', actions: [], dismissible: true, context: 'banner' })
    push({ level: 'warning', title: 'C', message: '', actions: [], dismissible: true, context: 'modal' })

    expect(useNotificationStore.getState().notifications).toHaveLength(3)
    dismissAll()
    expect(useNotificationStore.getState().notifications).toHaveLength(0)
  })

  it('dismissAll on empty is a no-op', () => {
    useNotificationStore.getState().dismissAll()
    expect(useNotificationStore.getState().notifications).toHaveLength(0)
  })
})
