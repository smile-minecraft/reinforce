import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { ApiError } from '../api/client'
import { useAuthStatus, useLogin, useRegister } from '../api/hooks/useAuth'

export function LoginPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { data: status } = useAuthStatus()
  const login = useLogin()
  const register = useRegister()

  const [mode, setMode] = useState<'login' | 'register'>('login')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [inviteCode, setInviteCode] = useState('')
  const [notice, setNotice] = useState<string | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)

  async function handleSubmit(event: FormEvent) {
    event.preventDefault()
    setErrorMsg(null)
    setNotice(null)
    try {
      if (mode === 'login') {
        await login.mutateAsync({ username, password })
        void navigate('/')
      } else {
        await register.mutateAsync({ invite_code: inviteCode, username, password })
        setNotice(t('auth.registeredSuccess'))
        setMode('login')
        setInviteCode('')
      }
    } catch (err) {
      setErrorMsg(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }

  return (
    <div className="mx-auto flex max-w-md flex-col px-4 py-12">
      <div className="rounded-xl border border-border bg-surface p-6">
        <h1 className="text-xl font-semibold text-ink">
          {mode === 'login' ? t('auth.titleSignIn') : t('auth.titleRegister')}
        </h1>
        <p className="mt-1.5 text-sm text-ink-muted">{t('auth.subtitleInvite')}</p>

        {status?.needs_owner_setup && (
          <div className="mt-4 rounded-lg bg-warn-soft p-3 text-sm text-warn">
            {t('auth.needsOwnerSetup')}
          </div>
        )}

        {notice && (
          <div className="mt-4 rounded-lg bg-ok-soft p-3 text-sm text-ok">{notice}</div>
        )}

        {errorMsg && (
          <div className="mt-4 rounded-lg bg-bad-soft p-3 text-sm text-bad">{errorMsg}</div>
        )}

        <form onSubmit={handleSubmit} className="mt-5 space-y-4">
          {mode === 'register' && (
            <label className="block">
              <span className="text-sm font-medium text-ink">{t('auth.inviteCode')}</span>
              <input
                type="text"
                required
                value={inviteCode}
                onChange={(e) => setInviteCode(e.target.value)}
                className="mt-1.5 block min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
              />
            </label>
          )}

          <label className="block">
            <span className="text-sm font-medium text-ink">{t('auth.username')}</span>
            <input
              type="text"
              required
              autoComplete="username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              className="mt-1.5 block min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
            />
          </label>

          <label className="block">
            <span className="text-sm font-medium text-ink">{t('auth.password')}</span>
            <input
              type="password"
              required
              autoComplete={mode === 'login' ? 'current-password' : 'new-password'}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="mt-1.5 block min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
            />
          </label>

          <button
            type="submit"
            disabled={login.isPending || register.isPending}
            className="flex min-h-11 w-full items-center justify-center rounded-lg bg-accent px-4 text-sm font-medium text-white hover:bg-accent-hover disabled:opacity-50"
          >
            {mode === 'login' ? t('auth.submitSignIn') : t('auth.submitRegister')}
          </button>
        </form>

        <button
          type="button"
          onClick={() => {
            setMode(mode === 'login' ? 'register' : 'login')
            setErrorMsg(null)
          }}
          className="mt-4 flex min-h-11 w-full items-center justify-center rounded-lg text-sm font-medium text-accent hover:bg-subtle"
        >
          {mode === 'login' ? t('auth.switchToRegister') : t('auth.switchToSignIn')}
        </button>
      </div>
    </div>
  )
}
