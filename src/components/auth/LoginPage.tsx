import { useState } from 'react'

import { LoginCard } from '@/components/auth/LoginCard'
import { LoginFooter } from '@/components/auth/LoginFooter'
import { LoginLogoStack } from '@/components/auth/LoginLogoStack'
import { LoginOptionsRow } from '@/components/auth/LoginOptionsRow'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'

const REMEMBER_KEY = 'login_remembered_username'

function getSavedUsername() {
  return localStorage.getItem(REMEMBER_KEY) ?? ''
}

function splitUsername(full: string): [string, string] {
  const idx = full.indexOf('@')
  if (idx === -1) return [full, '']
  return [full.slice(0, idx), full.slice(idx + 1)]
}

export function LoginPage() {
  const login = useAuthStore((s) => s.login)
  const isAuthPending = useAuthStore((s) => s.isAuthPending)
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const [userPrefix, userSuffix] = splitUsername(getSavedUsername())
  const [usernamePrefix, setUsernamePrefix] = useState(userPrefix)
  const [usernameSuffix, setUsernameSuffix] = useState(userSuffix)
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(() => !!localStorage.getItem(REMEMBER_KEY))
  const [error, setError] = useState('')

  async function handleSubmit(event: { preventDefault: () => void }) {
    event.preventDefault()
    const username = `${usernamePrefix.trim()}@${usernameSuffix.trim()}`
    try {
      setError('')
      await login(username, password)
      if (remember) {
        localStorage.setItem(REMEMBER_KEY, username)
      } else {
        localStorage.removeItem(REMEMBER_KEY)
      }
    } catch (err) {
      setPassword('')
      setError(err instanceof Error ? err.message : '登录失败，请重试')
    }
  }

  return (
    <div className="relative flex min-h-screen w-full flex-col items-center justify-center gap-6 px-6 overflow-hidden" style={{ background: 'linear-gradient(135deg, #fdf8ee 0%, #faf5f0 40%, #f0f4ff 100%)' }}>
      <div data-tauri-drag-region className="absolute inset-x-0 top-0 h-8 z-10" />
      {/* 背景光晕 */}
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -top-32 -left-32 h-[500px] w-[500px] rounded-full opacity-30" style={{ background: 'radial-gradient(circle, #DBAA22 0%, transparent 70%)', filter: 'blur(80px)' }} />
        <div className="absolute -bottom-40 -right-40 h-[600px] w-[600px] rounded-full opacity-20" style={{ background: 'radial-gradient(circle, #f59e0b 0%, transparent 70%)', filter: 'blur(100px)' }} />
        <div className="absolute top-1/2 left-1/3 h-[400px] w-[400px] -translate-y-1/2 rounded-full opacity-15" style={{ background: 'radial-gradient(circle, #818cf8 0%, transparent 70%)', filter: 'blur(90px)' }} />
      </div>
      <LoginLogoStack logoUrl={logoUrl} brandName={productName} />
      <LoginCard>
        <div className="flex flex-col gap-1.5">
          <div className="text-xl font-semibold text-foreground">登录到 {productName}</div>
          <div className="text-[0.8125rem] text-muted-foreground">使用企业账号继续</div>
        </div>
        <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="username-prefix">账号</Label>
            <div className="flex items-center gap-1.5">
              <Input
                id="username-prefix"
                aria-label="账号"
                placeholder="用户名"
                value={usernamePrefix}
                onChange={(e) => setUsernamePrefix(e.target.value)}
                autoComplete="username"
              />
              <span className="shrink-0 text-sm font-medium text-muted-foreground">@</span>
              <Input
                id="username-suffix"
                aria-label="企业编号"
                placeholder="企业编号"
                value={usernameSuffix}
                onChange={(e) => setUsernameSuffix(e.target.value)}
                autoComplete="off"
              />
            </div>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="password">密码</Label>
            <Input
              id="password"
              type="password"
              placeholder="请输入密码"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
          <LoginOptionsRow
            rememberSlot={
              <label className="flex items-center gap-2 text-[0.8125rem] text-foreground">
                <input
                  type="checkbox"
                  checked={remember}
                  onChange={(e) => setRemember(e.target.checked)}
                  className="h-4 w-4 shrink-0 cursor-pointer appearance-none rounded border border-border bg-background transition-colors checked:border-primary checked:bg-primary"
                  ref={(el) => {
                    if (!el) return
                    const update = () => {
                      el.style.backgroundImage = el.checked
                        ? `url("data:image/svg+xml,%3Csvg viewBox='0 0 16 16' fill='white' xmlns='http://www.w3.org/2000/svg'%3E%3Cpath d='M12.207 4.793a1 1 0 0 1 0 1.414l-5 5a1 1 0 0 1-1.414 0l-2-2a1 1 0 0 1 1.414-1.414L6.5 9.086l4.293-4.293a1 1 0 0 1 1.414 0z'/%3E%3C/svg%3E")`
                        : 'none'
                    }
                    update()
                    el.addEventListener('change', update)
                  }}
                />
                记住我
              </label>
            }
            onForget={() => {}}
          />
          {error ? (
            <div className="text-[0.8125rem] text-destructive">{error}</div>
          ) : null}
          <Button
            type="submit"
            disabled={isAuthPending}
            className="w-full rounded-full py-3 text-[0.9375rem] font-semibold"
          >
            {isAuthPending ? (
              <span className="flex items-center justify-center gap-2">
                <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none">
                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                </svg>
                登录中…
              </span>
            ) : '登录'}
          </Button>
          <div className="text-center text-xs text-muted-foreground">
            登录即代表同意《服务条款》与《隐私政策》
          </div>
        </form>
      </LoginCard>
      <LoginFooter text="AI 小家 v0.9.30 · © 仁励家网络科技(杭州)有限公司" />
    </div>
  )
}
