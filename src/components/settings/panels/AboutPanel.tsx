/**
 * @designSource design.pen#MQLyd appCard + 7s18f helpSec + lcRrf devSec
 */
import { ArrowRight } from 'lucide-react'

interface AboutPanelProps {
  appName: string
  version: string
  tenantName: string
  helpLinks: { label: string; onClick: () => void }[]
  devInfo: { label: string; value: string }[]
}

export function AboutPanel({ appName, version, tenantName, helpLinks, devInfo }: AboutPanelProps) {
  return (
    <>
      <div className="flex items-center justify-between gap-4 rounded-[14px] bg-secondary p-5">
        <div className="flex flex-col gap-1">
          <div className="text-base font-bold text-foreground">{appName}</div>
          <div className="text-[13px] text-muted-foreground">v{version} · {tenantName}</div>
        </div>
      </div>
      <section className="flex flex-col gap-4">
        <div className="text-sm font-semibold text-foreground">帮助与支持</div>
        <div className="flex flex-col gap-2">
          {helpLinks.map((l) => (
            <button
              key={l.label}
              type="button"
              onClick={l.onClick}
              className="flex items-center justify-between rounded-md px-3 py-2 text-sm text-foreground transition-colors hover:bg-muted"
            >
              <span>{l.label}</span>
              <ArrowRight className="h-3.5 w-3.5 text-muted-foreground" />
            </button>
          ))}
        </div>
      </section>
      <section className="flex flex-col gap-4">
        <div className="text-sm font-semibold text-foreground">开发者信息</div>
        <dl className="grid grid-cols-2 gap-y-2 text-[13px]">
          {devInfo.map((d) => (
            <div key={d.label} className="contents">
              <dt className="text-muted-foreground">{d.label}</dt>
              <dd className="text-foreground">{d.value}</dd>
            </div>
          ))}
        </dl>
      </section>
    </>
  )
}
