import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'

import { useNetworkStore } from '@/stores/networkStore'
import { useNotificationStore } from '@/stores/notificationStore'

export function useOfflineSendWarning() {
  const { t } = useTranslation()

  const warnIfOffline = useCallback(() => {
    if (useNetworkStore.getState().status !== 'offline') return
    useNotificationStore.getState().push({
      context: 'toast',
      level: 'warning',
      title: t('network.sendWhileOfflineTitle'),
      message: t('network.sendWhileOfflineDesc'),
      actions: [],
      dismissible: true,
      autoHide: 6,
    })
  }, [t])

  return { warnIfOffline }
}
