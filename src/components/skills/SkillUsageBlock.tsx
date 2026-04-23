/**
 * @designSource design.pen#MTvV8 useSec
 * @sizing title 15/600 + body 13 muted; gap 8
 */
interface SkillUsageBlockProps {
  text: string
}

export function SkillUsageBlock({ text }: SkillUsageBlockProps) {
  return (
    <section className="flex w-full flex-col gap-2">
      <div className="text-[15px] font-semibold text-foreground">使用说明</div>
      <p className="max-w-[880px] text-[13px] text-muted-foreground">{text}</p>
    </section>
  )
}
