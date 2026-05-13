/**
 * 把后端返回的 RFC 3339 UTC 字符串渲染为本地时间 "YYYY-MM-DD HH:MM:SS"。
 * 入参为空/非法时返回 null（调用方自行决定是否隐藏整行）。
 *
 * 抽象在这里独立维护，方便日后切换显示粒度（到分钟 / 到天）或时区策略，
 * 不需要散到各个调用方。
 */
export function formatSkillUpdatedAt(value: string | null | undefined): string | null {
  if (!value) return null
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return null
  const pad = (n: number) => String(n).padStart(2, '0')
  const y = date.getFullYear()
  const mo = pad(date.getMonth() + 1)
  const d = pad(date.getDate())
  const h = pad(date.getHours())
  const m = pad(date.getMinutes())
  const s = pad(date.getSeconds())
  return `${y}-${mo}-${d} ${h}:${m}:${s}`
}
