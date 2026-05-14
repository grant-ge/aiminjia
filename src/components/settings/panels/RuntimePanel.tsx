import { useEffect, useState } from 'react'

import { runtimeDiagnostics, type RuntimeDiagnostics } from '@/lib/tauri'

import { Button } from '@/components/ui/button'

const RESOLVER_LABEL: Record<RuntimeDiagnostics['activeResolver'], string> = {
  bundled: '内置（随安装包）',
  installed: '已升级（OSS 下载）',
  none: '不可用',
}

export function RuntimePanel() {
  const [data, setData] = useState<RuntimeDiagnostics | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const load = async () => {
    setLoading(true)
    setError(null)
    try {
      setData(await runtimeDiagnostics())
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  return (
    <section className="flex flex-col gap-4">
      <header>
        <h2 className="text-base font-semibold text-foreground">运行时</h2>
        <p className="mt-0.5 text-xs text-muted-foreground">
          AIjia 内置 Node、Python、uv 运行时。安装包内置一份，可通过 OSS 升级。
        </p>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error}
        </div>
      )}

      {data && (
        <dl className="grid grid-cols-[120px_1fr] gap-x-4 gap-y-2 rounded-md border border-border bg-muted/30 p-4 text-sm">
          <dt className="text-muted-foreground">来源</dt>
          <dd className="font-mono text-foreground">{RESOLVER_LABEL[data.activeResolver]}</dd>

          <dt className="text-muted-foreground">内置版本</dt>
          <dd className="font-mono text-foreground">{data.bundledVersion ?? '—'}</dd>

          <dt className="text-muted-foreground">升级版本</dt>
          <dd className="font-mono text-foreground">{data.installedVersion ?? '—'}</dd>

          <dt className="text-muted-foreground">Node</dt>
          <dd className="font-mono text-foreground">{data.node}</dd>

          <dt className="text-muted-foreground">Python</dt>
          <dd className="font-mono text-foreground">{data.python}</dd>

          <dt className="text-muted-foreground">uv</dt>
          <dd className="font-mono text-foreground">{data.uv}</dd>
        </dl>
      )}

      <Button variant="outline" onClick={() => void load()} disabled={loading} className="self-start">
        {loading ? '检查中…' : '重新检查'}
      </Button>
    </section>
  )
}
