/**
 * Notification store — manages all user-facing notifications.
 * Based on tech-architecture.md §11.8
 */
import { create } from 'zustand'

export type NotificationLevel = 'info' | 'success' | 'warning' | 'error'
export type NotificationContext = 'toast' | 'inline' | 'banner' | 'modal'

export interface NotificationAction {
  label: string
  action: () => void
  primary?: boolean
}

export interface Notification {
  id: string
  level: NotificationLevel
  title: string
  message: string
  actions: NotificationAction[]
  dismissible: boolean
  autoHide?: number // seconds
  persistent?: boolean
  context: NotificationContext
  createdAt: number
}

interface NotificationState {
  notifications: Notification[]

  // Actions
  push: (notification: Omit<Notification, 'id' | 'createdAt'>) => void
  dismiss: (id: string) => void
  dismissAll: () => void
}

let notificationCounter = 0

export const useNotificationStore = create<NotificationState>((set) => ({
  notifications: [],

  push: (notification) => {
    const id = `notif_${++notificationCounter}_${Date.now()}`
    const full: Notification = {
      ...notification,
      id,
      createdAt: Date.now(),
    }

    set((state) => ({
      notifications: [...state.notifications, full],
    }))

    // Auto-dismiss if configured
    if (notification.autoHide && notification.autoHide > 0) {
      setTimeout(() => {
        set((state) => ({
          notifications: state.notifications.filter((n) => n.id !== id),
        }))
      }, notification.autoHide * 1000)
    }
  },

  dismiss: (id) =>
    set((state) => ({
      notifications: state.notifications.filter((n) => n.id !== id),
    })),

  dismissAll: () => set({ notifications: [] }),
}))
