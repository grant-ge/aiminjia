import { useMemo, useState } from 'react'

import type { Question } from '@/lib/tauri'
import { cancelUserInteraction, submitUserInteraction } from '@/lib/tauri'

interface Props {
  interactionId: string
  questions: Question[]
  onClose: () => void
}

const OTHER_VALUE = '__other__'

export function AskUserQuestionDialog({ interactionId, questions, onClose }: Props) {
  const [answers, setAnswers] = useState<Record<string, string[]>>({})
  const [customInputs, setCustomInputs] = useState<Record<string, string>>({})
  const [submitting, setSubmitting] = useState(false)

  const canSubmit = useMemo(
    () => questions.every((question) => (answers[question.question]?.length ?? 0) > 0),
    [answers, questions],
  )

  function toggleOption(questionText: string, label: string, multiSelect: boolean) {
    setAnswers((prev) => {
      const current = prev[questionText] ?? []
      if (multiSelect) {
        return {
          ...prev,
          [questionText]: current.includes(label)
            ? current.filter((item) => item !== label)
            : [...current, label],
        }
      }
      return { ...prev, [questionText]: [label] }
    })
  }

  async function handleSubmit() {
    if (!canSubmit || submitting) return
    setSubmitting(true)
    try {
      const flatAnswers: Record<string, string> = {}
      for (const question of questions) {
        const selected = answers[question.question] ?? []
        const custom = (customInputs[question.question] ?? '').trim()
        const values = selected.map((value) => (value === OTHER_VALUE ? custom || 'Other' : value))
        flatAnswers[question.question] = values.join(', ')
      }
      await submitUserInteraction(interactionId, { answers: flatAnswers })
      onClose()
    } finally {
      setSubmitting(false)
    }
  }

  async function handleCancel() {
    if (submitting) return
    setSubmitting(true)
    try {
      await cancelUserInteraction(interactionId, 'User dismissed the question dialog.')
      onClose()
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-gray-950/35 px-4 backdrop-blur-sm">
      <div className="w-full max-w-xl rounded-lg border border-border bg-background p-6 shadow-[var(--shadow-modal)]">
        <div className="mb-5">
          <div className="text-xs font-semibold uppercase tracking-[0.22em] text-muted-foreground">AI needs your input</div>
          <h2 className="mt-1 text-lg font-semibold text-foreground">AI 向你提问</h2>
          <p className="mt-1 text-sm text-muted-foreground">选择最贴近你意图的答案，或使用“其他”补充说明。</p>
        </div>

        <div className="max-h-[62vh] space-y-5 overflow-y-auto pr-1">
          {questions.map((question) => {
            const selectedValues = answers[question.question] ?? []
            const hasOther = selectedValues.includes(OTHER_VALUE)
            return (
              <section key={question.question} className="space-y-3 rounded-xl border border-border/80 bg-muted/20 p-4">
                <div>
                  <div className="text-xs font-medium text-muted-foreground">{question.header}</div>
                  <div className="mt-1 text-sm font-semibold text-foreground">{question.question}</div>
                </div>

                <div className="grid gap-2 sm:grid-cols-2">
                  {question.options.map((option) => {
                    const selected = selectedValues.includes(option.label)
                    return (
                      <button
                        key={option.label}
                        type="button"
                        onClick={() => toggleOption(question.question, option.label, !!question.multiSelect)}
                        className={`rounded-xl border px-3 py-2 text-left text-sm transition-colors ${
                          selected
                            ? 'border-primary bg-primary/10 text-primary'
                            : 'border-border bg-background text-foreground hover:bg-muted'
                        }`}
                      >
                        <div className="font-medium">{option.label}</div>
                        <div className="mt-1 text-xs text-muted-foreground">{option.description}</div>
                        {option.preview ? (
                          <pre className="mt-2 max-h-24 overflow-auto rounded-md bg-muted p-2 text-xs text-muted-foreground">{option.preview}</pre>
                        ) : null}
                      </button>
                    )
                  })}
                  <button
                    type="button"
                    onClick={() => toggleOption(question.question, OTHER_VALUE, !!question.multiSelect)}
                    className={`rounded-xl border px-3 py-2 text-left text-sm transition-colors ${
                      hasOther
                        ? 'border-primary bg-primary/10 text-primary'
                        : 'border-border bg-background text-foreground hover:bg-muted'
                    }`}
                  >
                    <div className="font-medium">其他</div>
                    <div className="mt-1 text-xs text-muted-foreground">输入自定义回答</div>
                  </button>
                </div>

                {hasOther ? (
                  <input
                    type="text"
                    value={customInputs[question.question] ?? ''}
                    onChange={(event) =>
                      setCustomInputs((prev) => ({ ...prev, [question.question]: event.target.value }))
                    }
                    placeholder="请输入你的自定义回答"
                    className="w-full rounded-lg border border-border bg-background px-3 py-2 text-sm outline-none focus:border-primary"
                  />
                ) : null}
              </section>
            )
          })}
        </div>

        <div className="mt-6 flex justify-end gap-2">
          <button
            type="button"
            onClick={handleCancel}
            disabled={submitting}
            className="rounded-lg border border-border px-4 py-2 text-sm text-muted-foreground hover:bg-muted disabled:opacity-60"
          >
            取消
          </button>
          <button
            type="button"
            onClick={handleSubmit}
            disabled={!canSubmit || submitting}
            className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-60"
          >
            提交回答
          </button>
        </div>
      </div>
    </div>
  )
}
