import type { RenderPeerBanner } from '@/hooks/useTurnRenderModel'

interface Props {
  banners: RenderPeerBanner[]
}

export function PeerMessageBanner({ banners }: Props) {
  if (banners.length === 0) return null

  const peerItems = banners.filter((b) => b.kind === 'peer')
  const taskItems = banners.filter((b) => b.kind === 'task')

  return (
    <div className="flex w-full flex-col gap-1.5">
      {peerItems.length > 0 && (
        <div className="rounded-lg border border-border bg-muted px-3 py-2 text-sm">
          <div className="mb-1.5 flex items-center gap-1.5 font-medium text-foreground">
            <span>🔔</span>
            <span>团队消息</span>
          </div>
          <div className="flex flex-col gap-1.5">
            {peerItems.map((b, i) => (
              <div key={i}>
                <span className="font-medium text-foreground">
                  {b.from} → {b.to ?? 'Lead'}
                </span>
                <p className="mt-0.5 text-muted-foreground">{b.body}</p>
              </div>
            ))}
          </div>
        </div>
      )}
      {taskItems.map((b, i) => (
        <div key={i} className="rounded-lg border border-border bg-muted px-3 py-2 text-sm">
          <div className="mb-1 flex items-center gap-1.5 font-medium text-foreground">
            <span>✅</span>
            <span>子任务完成</span>
          </div>
          <div>
            <span className="font-medium text-foreground">{b.agent}</span>
            {b.body && <p className="mt-0.5 text-muted-foreground">{b.body}</p>}
          </div>
        </div>
      ))}
    </div>
  )
}
