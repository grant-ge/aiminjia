export function FullscreenLoader() {
  return (
    <div
      className="fixed inset-0 flex items-center justify-center bg-background text-foreground"
      style={{ animation: 'fadeInDelayed 0.2s ease forwards', animationDelay: '300ms', opacity: 0 }}
    >
      <style>{`@keyframes fadeInDelayed { to { opacity: 1; } }`}</style>
      <div className="flex flex-col items-center gap-3">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-muted border-t-primary" />
        <p className="text-sm text-muted-foreground">正在恢复登录状态...</p>
      </div>
    </div>
  )
}
