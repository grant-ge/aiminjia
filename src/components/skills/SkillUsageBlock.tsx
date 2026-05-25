/**
 * @designSource design.pen#MTvV8 useSec
 * @sizing title 15/600 + body 13 muted; gap 8
 */
interface SkillUsageBlockProps {
  usageSteps: string[]
  notes?: string[]
}

export function SkillUsageBlock({ usageSteps, notes }: SkillUsageBlockProps) {
  return (
    <section className="flex w-full max-w-[880px] flex-col gap-6">
      <div className="flex flex-col gap-2">
        <div className="text-md font-semibold text-foreground">使用说明</div>
        <ol className="flex list-decimal flex-col gap-1.5 pl-5 text-sm text-muted-foreground">
          {usageSteps.map((step, i) => (
            <li key={i}>{step}</li>
          ))}
        </ol>
      </div>
      {notes && notes.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="text-md font-semibold text-foreground">注意事项</div>
          <ul className="flex list-disc flex-col gap-1.5 pl-5 text-sm text-muted-foreground">
            {notes.map((note, i) => (
              <li key={i}>{note}</li>
            ))}
          </ul>
        </div>
      )}
    </section>
  )
}
