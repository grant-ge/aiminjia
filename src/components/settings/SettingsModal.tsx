import { useState } from 'react'

import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useAuthStore } from '@/stores/authStore'
import { useUiStore } from '@/stores/uiStore'

type MainTab = 'account' | 'general' | 'about' | 'usage'

export function SettingsModal() {
  const currentTab = useUiStore((state) => state.settingsModal)
  const closeSettings = useUiStore((state) => state.closeSettings)
  const logout = useAuthStore((state) => state.logout)
  const user = useAuthStore((state) => state.user)
  const tenant = useAuthStore((state) => state.tenant)
  const [isSubmitting, setIsSubmitting] = useState(false)

  if (!currentTab) return null

  const activeTab = (currentTab as MainTab)

  const handleLogout = async () => {
    setIsSubmitting(true)
    try {
      await logout()
      closeSettings()
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) closeSettings() }}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>设置</DialogTitle>
          <DialogDescription>管理账号、通用偏好与关于信息。</DialogDescription>
        </DialogHeader>
        <div className="flex gap-6">
          <aside className="w-44 shrink-0 space-y-1">
            {([
              { id: 'account', label: '账号' },
              { id: 'general', label: '通用' },
              { id: 'about', label: '关于' },
              { id: 'usage', label: '使用情况' },
            ] as const).map((item) => (
              <Button
                key={item.id}
                className="w-full justify-start"
                variant={activeTab === item.id ? 'secondary' : 'ghost'}
                onClick={() => useUiStore.getState().openSettings(item.id)}
              >
                {item.label}
              </Button>
            ))}
          </aside>
          <div className="min-w-0 flex-1 space-y-4">
            {activeTab === 'account' && (
              <div className="space-y-4">
                <div className="rounded-lg border border-border bg-card p-4">
                  <div className="text-sm font-medium text-foreground">{user?.name ?? user?.username ?? '未登录'}</div>
                  <div className="mt-1 text-sm text-muted-foreground">{tenant?.name ?? '当前未绑定租户'}</div>
                </div>
                <AlertDialog>
                  <AlertDialogTrigger asChild>
                    <Button variant="destructive">退出登录</Button>
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogTitle>确认退出登录？</AlertDialogTitle>
                      <AlertDialogDescription>退出后将返回登录页，本次主动退出不会保留当前现场。</AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                      <AlertDialogCancel>取消</AlertDialogCancel>
                      <AlertDialogAction disabled={isSubmitting} onClick={() => void handleLogout()}>退出登录</AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              </div>
            )}
            {activeTab === 'general' && (
              <div className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground">
                通用设置将在后续任务中继续细化。
              </div>
            )}
            {activeTab === 'about' && (
              <div className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground">
                Skill-First shell 已启用，当前版本信息会在后续验收中补齐。
              </div>
            )}
            {activeTab === 'usage' && (
              <div className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground">
                用量统计面板将在后续任务中补齐。
              </div>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
