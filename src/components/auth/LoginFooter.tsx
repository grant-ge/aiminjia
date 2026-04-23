/**
 * @designSource design.pen#wJSL6
 * @sizing fontSize 12 muted
 */
interface LoginFooterProps {
  text: string
}

export function LoginFooter({ text }: LoginFooterProps) {
  return <div className="text-[12px] text-muted-foreground">{text}</div>
}
