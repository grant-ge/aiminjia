import { useTranslation } from 'react-i18next'
import { Modal } from './Modal'
import { Button } from './Button'

interface WhatsNewModalProps {
  open: boolean
  onClose: () => void
  version: string
  changes: string[]
}

export function WhatsNewModal({ open, onClose, version, changes }: WhatsNewModalProps) {
  const { t } = useTranslation()

  const footer = (
    <Button variant="primary" onClick={onClose}>
      {t('changelog.ok')}
    </Button>
  )

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t('changelog.whatsNew', { version })}
      footer={footer}
      size="md"
    >
      {changes.length > 0 ? (
        <ul className="space-y-2 pl-5 list-disc" style={{ color: 'var(--color-text-secondary)' }}>
          {changes.map((change, i) => (
            <li key={i} className="text-sm leading-relaxed whitespace-pre-wrap break-words">{change}</li>
          ))}
        </ul>
      ) : (
        <p className="text-sm" style={{ color: 'var(--color-text-muted)' }}>
          {t('changelog.noChanges')}
        </p>
      )}
    </Modal>
  )
}
