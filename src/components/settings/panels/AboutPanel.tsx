/**
 * @designSource copied from Wukong about settings page, adapted to AI 小家 branding.
 */
import { useState } from 'react'
import { ArrowUpRight } from 'lucide-react'

import { Switch } from '@/components/common/Switch'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

const UX_PROGRAM_STORAGE_KEY = 'about.user-experience-program'

interface AboutPanelLinks {
  customerService: () => void
  productSuggestion: () => void
  privacyPolicy: () => void
  terms: () => void
}

interface AboutPanelProps {
  appName: string
  version: string
  copyright: string
  logoUrl: string
  onCheckUpdate: () => void
  onUploadLogs: () => void
  onResetData: () => void
  links: AboutPanelLinks
}

function readExperienceProgramDefault() {
  try {
    const stored = localStorage.getItem(UX_PROGRAM_STORAGE_KEY)
    if (stored === null) return true
    return stored === 'on'
  } catch {
    return true
  }
}

function writeExperienceProgram(enabled: boolean) {
  try {
    localStorage.setItem(UX_PROGRAM_STORAGE_KEY, enabled ? 'on' : 'off')
  } catch {
    // localStorage can be unavailable in restricted environments.
  }
}

function PillButton({
  children,
  onClick,
  danger = false,
  disabled = false,
}: {
  children: string
  onClick: () => void
  danger?: boolean
  disabled?: boolean
}) {
  return (
    <Button
      type="button"
      onClick={onClick}
      disabled={disabled}
      variant={danger ? 'destructive' : 'outline'}
      className={cn(
        'h-10 rounded-[12px] px-5 text-sm font-semibold',
        disabled && 'cursor-not-allowed opacity-60',
      )}
    >
      {children}
    </Button>
  )
}

function ExternalLinkButton({
  children,
  onClick,
  highlight = false,
  disabled = false,
}: {
  children: string
  onClick: () => void
  highlight?: boolean
  disabled?: boolean
}) {
  return (
    <Button
      type="button"
      onClick={onClick}
      disabled={disabled}
      variant="ghost"
      className={cn(
        'h-auto justify-start rounded-none px-0 py-2 text-left text-base font-semibold hover:bg-transparent hover:opacity-75',
        highlight ? 'text-primary hover:text-primary' : 'text-foreground hover:text-foreground',
        disabled && 'cursor-not-allowed opacity-60 hover:opacity-60',
      )}
    >
      <span>{children}</span>
      <ArrowUpRight className="h-4 w-4" strokeWidth={2.2} />
    </Button>
  )
}

function ComingSoonBadge() {
  return (
    <span className="rounded bg-muted px-1.5 py-0.5 text-[0.625rem] font-medium text-muted-foreground">
      即将支持
    </span>
  )
}

export function AboutPanel({
  appName,
  version,
  copyright,
  logoUrl,
  onCheckUpdate,
  onUploadLogs,
  onResetData,
  links,
}: AboutPanelProps) {
  const [experienceProgramEnabled, setExperienceProgramEnabled] = useState(readExperienceProgramDefault)

  const toggleExperienceProgram = () => {
    setExperienceProgramEnabled((enabled) => {
      const nextEnabled = !enabled
      writeExperienceProgram(nextEnabled)
      return nextEnabled
    })
  }

  return (
    <div className="flex flex-col gap-4 text-foreground">
      <section className="flex items-center justify-between gap-6">
        <div className="flex min-w-0 items-start gap-4">
          <img
            src={logoUrl}
            alt={`${appName} 图标`}
            className="h-16 w-16 shrink-0 rounded-[14px] border-border bg-card object-cover"
          />
          <div className="flex min-w-0 flex-col gap-1.5 pt-1">
            <div className="text-base font-bold leading-none text-foreground">{appName}</div>
            <div className="text-sm leading-none text-muted-foreground">版本 {version}</div>
            <div className="text-sm leading-none text-muted-foreground">版权公告：{copyright}</div>
          </div>
        </div>
        <PillButton onClick={onCheckUpdate}>检查更新</PillButton>
      </section>

      <div className="h-px bg-border mb-2" />

      <section className="flex flex-col gap-3">
        <div className="text-xl font-bold tracking-tight text-foreground">帮助与反馈</div>

        <div className="flex items-center justify-between gap-6 opacity-60">
          <div className="flex min-w-0 flex-col gap-2">
            <div className="flex items-center gap-2">
              <span className="text-base font-semibold text-foreground">用户体验改进计划</span>
              <ComingSoonBadge />
            </div>
            <p className="max-w-[650px] text-sm leading-5 text-muted-foreground">
              诚邀您加入用户体验改进计划。开启后，我们将收集您的对话记录、任务执行记录、设备与网络信息，相关数据会在脱敏处理后用于优化产品体验。您可随时开启或关闭该计划，详情请见
              <Button
                type="button"
                onClick={links.privacyPolicy}
                disabled
                variant="link"
                className="h-auto cursor-not-allowed rounded-none p-0 align-baseline text-sm font-semibold text-primary opacity-60"
              >
                隐私权政策
              </Button>
              。
            </p>
          </div>
          <Switch
            aria-label="用户体验改进计划"
            checked={experienceProgramEnabled}
            onCheckedChange={toggleExperienceProgram}
            disabled
          />
        </div>

        <div className="flex flex-col items-start gap-1 opacity-60">
          <ExternalLinkButton onClick={links.customerService} disabled>在线客服</ExternalLinkButton>
          <ExternalLinkButton onClick={links.productSuggestion} disabled>产品建议</ExternalLinkButton>
          <ExternalLinkButton onClick={links.privacyPolicy} highlight disabled>隐私政策</ExternalLinkButton>
          <ExternalLinkButton onClick={links.terms} disabled>服务条款</ExternalLinkButton>
        </div>
      </section>

      <div className="h-px bg-border mb-2" />

      <section className="flex flex-col gap-3 pb-2">
        <div className="text-xl font-bold tracking-tight text-foreground">开发者模式</div>

        <div className="flex items-center justify-between gap-6 opacity-60">
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="text-base font-semibold text-foreground">日志上传</span>
              <ComingSoonBadge />
            </div>
            <div className="text-sm text-muted-foreground">上传诊断日志以协助排查问题</div>
          </div>
          <PillButton onClick={onUploadLogs} disabled>上传日志</PillButton>
        </div>

        <div className="flex items-center justify-between gap-6 opacity-60">
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="text-base font-semibold text-foreground">重置</span>
              <ComingSoonBadge />
            </div>
            <div className="text-sm text-muted-foreground">清除本地缓存</div>
          </div>
          <PillButton onClick={onResetData} danger disabled>重置</PillButton>
        </div>
      </section>
    </div>
  )
}
