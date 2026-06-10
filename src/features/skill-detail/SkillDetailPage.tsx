import { ArrowLeft } from 'lucide-react'
import { useState } from 'react'

import { Switch } from '@/components/common/Switch'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillActionBar } from '@/components/skills/SkillActionBar'
import { SkillDetailHero } from '@/components/skills/SkillDetailHero'
import { SkillMetaRow } from '@/components/skills/SkillMetaRow'
import { SkillUsageBlock } from '@/components/skills/SkillUsageBlock'
import { localizeSkill } from '@/lib/skillLocalization'
import {
  canToggleSkillEnablement,
  isBuiltinSkill,
  isMarketSkill,
  isSkillEnabled,
} from '@/lib/skillAvailability'
import { useNotificationStore } from '@/stores/notificationStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { formatSkillUpdatedAt } from './formatSkillUpdatedAt'

interface SkillDetailPageProps {
  skillId: string
}

export function SkillDetailPage({ skillId }: SkillDetailPageProps) {
  const skill = useSkillStore((s) => s.getById(skillId))
  const setSkillEnabled = useSkillStore((s) => s.setSkillEnabled)
  const pushNotification = useNotificationStore((s) => s.push)
  const setRoute = useUiStore((s) => s.setRoute)
  const setPendingSkill = useUiStore((s) => s.setPendingSkill)
  const [isChangingEnabled, setIsChangingEnabled] = useState(false)

  const goToSkillCenter = () => setRoute({ kind: 'skill-center' })

  const backButton = (
    <button
      type="button"
      aria-label="返回技能中心"
      onClick={goToSkillCenter}
      className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
    >
      <ArrowLeft className="h-4 w-4" />
    </button>
  )

  if (!skill) {
    return (
      <PageSectionShell
        topBar={
          <PageTopBar
            variant="breadcrumb"
            leading={backButton}
            breadcrumbs={[
              { label: '技能中心', onClick: goToSkillCenter },
              { label: '未知技能', current: true },
            ]}
          />
        }
      >
        <div className="text-sm text-muted-foreground">技能不存在或尚未加载。</div>
      </PageSectionShell>
    )
  }

  const localized = localizeSkill(skill)
  const enabled = isSkillEnabled(skill)
  const manageable = canToggleSkillEnablement(skill)
  const market = isMarketSkill(skill)
  const builtin = isBuiltinSkill(skill)
  const trigger = skill.triggerText?.trim() || `/${skill.id}`
  const updatedAt = formatSkillUpdatedAt(skill.updatedAt)

  const handleSetEnabled = async (nextEnabled: boolean): Promise<boolean> => {
    if (!manageable) return false
    setIsChangingEnabled(true)
    try {
      await setSkillEnabled(skill.id, nextEnabled)
      return true
    } catch (err) {
      pushNotification({
        level: 'error',
        title: nextEnabled ? '开启技能失败' : '关闭技能失败',
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
      return false
    } finally {
      setIsChangingEnabled(false)
    }
  }

  const handleUseSkill = async () => {
    if (manageable && !enabled) {
      const enabledForUse = await handleSetEnabled(true)
      if (!enabledForUse) return
    }
    setPendingSkill({
      id: skill.id,
      label: localized.name,
      trigger,
    })
    setRoute({ kind: 'home' })
  }

  const primaryLabel = market ? '添加并使用' : enabled ? '使用' : '开启并使用'
  const secondaryLabel = manageable ? (enabled ? '关闭' : '保持关闭') : undefined
  const handleSecondary = manageable
    ? enabled
      ? () => void handleSetEnabled(false)
      : goToSkillCenter
    : undefined

  return (
    <PageSectionShell
      topBar={
        <PageTopBar
          variant="breadcrumb"
          leading={backButton}
          breadcrumbs={[
            { label: '技能中心', onClick: goToSkillCenter },
            { label: localized.name, current: true },
          ]}
        />
      }
    >
      <SkillDetailHero
        title={localized.name}
        subtitle={localized.description || `通过命令 ${trigger} 快速调用`}
        actionBar={
          <div className="flex flex-wrap items-center justify-end gap-2.5">
            {manageable ? (
              <div className="flex items-center gap-2 rounded-md border border-border bg-card px-2.5 py-1.5 text-sm text-muted-foreground">
                <span>{enabled ? '已开启' : '已关闭'}</span>
                <Switch
                  checked={enabled}
                  disabled={isChangingEnabled}
                  aria-label={`${localized.name} 技能开关`}
                  onCheckedChange={(next) => void handleSetEnabled(next)}
                />
              </div>
            ) : null}
            <SkillActionBar
              primaryLabel={primaryLabel}
              primaryDisabled={isChangingEnabled}
              onPrimary={() => void handleUseSkill()}
              secondaryLabel={secondaryLabel}
              onSecondary={handleSecondary}
            />
          </div>
        }
      />

      <SkillMetaRow
        items={[
          {
            label: '来源',
            value: market ? '市场' : builtin ? 'AI 小家内置' : '已安装',
          },
          { label: '调用命令', value: trigger },
          ...(manageable ? [{ label: '状态', value: enabled ? '已开启' : '已关闭' }] : []),
          ...(updatedAt ? [{ label: '更新时间', value: updatedAt }] : []),
        ]}
      />

      <SkillUsageBlock
        usageSteps={[
          `点击右上角“${primaryLabel}”后，会回到对话首页并把技能 chip 放入输入框。`,
          '按需要补充上下文，可以继续输入任务要求或添加文件作为附件。',
          '发送后，AI 小家会按该技能的规则处理本轮请求。',
          enabled || !manageable
            ? `也可以在任意对话输入框手动输入 ${trigger} 加具体要求来触发。`
            : '当前技能关闭时不会出现在输入框技能选择或 slash 快捷入口中。',
        ]}
        notes={[
          '技能 chip 只对当前这一轮消息生效，发送后会自动清除。',
          '关闭技能后，它仍保留在管理页和详情页，但不会进入后续对话上下文。',
          market
            ? '市场技能来自企业下发或可安装来源；安装后会进入已安装管理视图。'
            : builtin
              ? '内置技能随客户端初始化和官方更新维护，可在这里按账号关闭或开启。'
              : '本地安装技能可以重新导入新版 SKILL 包完成更新。',
        ]}
      />
    </PageSectionShell>
  )
}
