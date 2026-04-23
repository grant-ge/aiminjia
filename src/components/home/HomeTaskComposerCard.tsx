import { Globe, Mic, Plus, Sparkles } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'

const QUICK_SKILLS = ['财务报告', '文案报告', '作业研究', '文件整理', '网站摘要', '长期代办']

export function HomeTaskComposerCard() {
  return (
    <section className="flex w-full flex-col items-center">
      <div className="w-full rounded-[28px] border border-[#ece6da] bg-white shadow-[0_18px_48px_rgba(41,31,12,0.08)]">
        <div className="px-5 pb-4 pt-5">
          <Textarea
            placeholder="描述目标、补充信息，或输入 / 选择你要调用的技能来开始。"
            className="min-h-[132px] resize-none border-0 bg-transparent px-0 py-0 text-[15px] leading-7 shadow-none placeholder:text-[#a29a8a] focus-visible:ring-0"
          />
        </div>

        <div className="border-t border-[#f1ecdf] px-4 py-3">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              variant="ghost"
              className="h-8 rounded-full border border-[#ebe3d4] bg-[#faf7f1] px-3 text-xs font-medium text-[#6c624d] hover:bg-[#f3ecde] hover:text-[#4b4437]"
            >
              <Plus className="size-3.5" />
              技能
            </Button>
            <Button
              type="button"
              variant="ghost"
              className="h-8 rounded-full border border-[#ebe3d4] bg-white px-3 text-xs font-medium text-[#7a715f] hover:bg-[#f7f2e8] hover:text-[#4b4437]"
            >
              <Sparkles className="size-3.5" />
              生成式任务代理
            </Button>
            <Button
              type="button"
              variant="ghost"
              className="h-8 rounded-full border border-[#ebe3d4] bg-white px-3 text-xs font-medium text-[#7a715f] hover:bg-[#f7f2e8] hover:text-[#4b4437]"
            >
              <Globe className="size-3.5" />
              Desktop
            </Button>

            <div className="ml-auto flex items-center gap-2">
              <button
                type="button"
                aria-label="语音输入"
                className="flex size-8 items-center justify-center rounded-full text-[#8e836f] transition-colors hover:bg-[#f5efe2] hover:text-[#5f5442]"
              >
                <Mic className="size-4" />
              </button>
              <button
                type="button"
                aria-label="发送任务"
                className="flex size-8 items-center justify-center rounded-full bg-[#f0ece4] text-[#8f8572] transition-colors hover:bg-[#dbd5ca] hover:text-[#5a5042]"
              >
                <Plus className="size-4 rotate-45" />
              </button>
            </div>
          </div>
        </div>
      </div>

      <div className="mt-4 flex w-full flex-wrap items-center gap-2">
        {QUICK_SKILLS.map((label, index) => (
          <button
            key={label}
            type="button"
            className={
              index === 0
                ? 'rounded-full bg-[#fff5d6] px-3 py-1.5 text-xs font-medium text-[#b37a00] transition-colors hover:bg-[#fde9aa]'
                : 'rounded-full border border-[#ebe3d4] bg-white px-3 py-1.5 text-xs font-medium text-[#7a715f] transition-colors hover:bg-[#f7f2e8] hover:text-[#4b4437]'
            }
          >
            {label}
          </button>
        ))}
      </div>
    </section>
  )
}
