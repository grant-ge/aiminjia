/**
 * @designSource design.pen#TtxTY/HSE9l/ZK6ey
 * @sizing fontSize 14 color foreground lineHeight ~1.5
 */
interface AiSegmentTextProps {
  text: string
}

export function AiSegmentText({ text }: AiSegmentTextProps) {
  return <div className="text-sm leading-[1.55] text-foreground">{text}</div>
}
