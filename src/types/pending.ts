export type PendingSource = 'app' | 'im-dingtalk'

export interface PendingAttachment {
  id: string
  filePath: string
  mime?: string | null
  sizeBytes?: number | null
}

export interface PendingItem {
  id: string
  source: PendingSource
  text: string
  senderNick?: string | null
  attachments: PendingAttachment[]
  receivedAt: string
}

export interface PendingSnapshotPayload {
  sessionId: string
  items: PendingItem[]
}

export interface PendingQueuedPayload {
  sessionId: string
  item: PendingItem
}

export interface PendingDrainedPayload {
  sessionId: string
  drainedIds: string[]
}

export interface PendingRemovedPayload {
  sessionId: string
  itemId: string
}
