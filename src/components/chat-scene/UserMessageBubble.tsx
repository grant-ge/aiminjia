/**
 * @designSource design.pen#1JNrw bubble/adaptive-max-80
 * @sizing r-16 padding [12,16] bg primary fg primary-foreground; align right; max-w 80%
 */
interface UserMessageBubbleProps {
  text: string
}

export function UserMessageBubble({ text }: UserMessageBubbleProps) {
  return (
    <div className="flex w-full justify-end">
      <div
        data-testid="user-bubble"
        className="max-w-[80%] rounded-2xl bg-primary px-4 py-3 text-sm text-primary-foreground"
      >
        {text}
      </div>
    </div>
  )
}
