import {
  BookOpen,
  ClipboardCheck,
  FileOutput,
  Globe,
  GraduationCap,
  LogIn,
  LogOut,
  Monitor,
  Moon,
  Sun,
  Timer,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link, NavLink, Outlet, useLocation } from 'react-router'
import { isMockMode } from '../api/client'
import { useAuthStatus, useLogout, useMeta } from '../api/hooks/useAuth'
import { SUPPORTED_LANGUAGES, type Language } from '../i18n'
import { useTheme, type ThemePreference } from './themeContext'

const THEME_ORDER: ThemePreference[] = ['system', 'light', 'dark']

export function AppShell() {
  const { t, i18n } = useTranslation()
  const { preference, setPreference } = useTheme()
  const { data: authStatus } = useAuthStatus()
  const { data: meta } = useMeta()
  const logout = useLogout()
  const location = useLocation()
  const mockActive = isMockMode()

  const needsSignIn = authStatus?.auth_mode === 'invite' && !authStatus.authenticated
  const isFullscreenSession = location.pathname.startsWith('/study/session/')

  function toggleLanguage() {
    const idx = SUPPORTED_LANGUAGES.indexOf(i18n.language as Language)
    const next = SUPPORTED_LANGUAGES[(idx + 1) % SUPPORTED_LANGUAGES.length]
    void i18n.changeLanguage(next)
  }

  function cycleTheme() {
    const idx = THEME_ORDER.indexOf(preference)
    setPreference(THEME_ORDER[(idx + 1) % THEME_ORDER.length])
  }

  const ThemeIcon = preference === 'dark' ? Moon : preference === 'light' ? Sun : Monitor
  const themeLabel =
    preference === 'dark'
      ? t('app.themeDark')
      : preference === 'light'
        ? t('app.themeLight')
        : t('app.themeSystem')

  if (isFullscreenSession) {
    return (
      <div className="min-h-dvh bg-canvas text-ink">
        <Outlet />
      </div>
    )
  }

  return (
    <div className="min-h-dvh bg-canvas text-ink lg:flex">
      {/* Desktop / iPad Landscape Navigation Rail */}
      <aside className="hidden lg:flex lg:w-60 lg:shrink-0 lg:flex-col lg:border-r lg:border-border lg:bg-surface lg:sticky lg:top-0 lg:h-dvh">
        <div className="flex items-center justify-between border-b border-border px-5 py-4">
          <Link to="/" className="flex items-center gap-2.5">
            <img src="/favicon.svg" alt="" className="h-7 w-7 rounded-lg" />
            <span className="text-base font-semibold tracking-tight text-ink">{t('app.name')}</span>
          </Link>
          {meta?.version && (
            <span className="rounded bg-subtle px-1.5 py-0.5 font-mono text-[11px] text-ink-muted">
              {meta.version}
            </span>
          )}
        </div>

        <nav className="flex flex-1 flex-col gap-1 p-3" aria-label="Main navigation">
          <NavLink
            to="/"
            end
            className={({ isActive }) =>
              `flex min-h-11 items-center gap-3 rounded-lg px-3 text-sm font-medium transition-colors ${
                isActive
                  ? 'bg-accent-soft text-accent'
                  : 'text-ink-muted hover:bg-subtle hover:text-ink'
              }`
            }
          >
            <BookOpen className="h-4 w-4 shrink-0" />
            <span>{t('nav.reading')}</span>
          </NavLink>

          <NavLink
            to="/study"
            className={({ isActive }) =>
              `flex min-h-11 items-center gap-3 rounded-lg px-3 text-sm font-medium transition-colors ${
                isActive
                  ? 'bg-accent-soft text-accent'
                  : 'text-ink-muted hover:bg-subtle hover:text-ink'
              }`
            }
          >
            <GraduationCap className="h-4 w-4 shrink-0" />
            <span>{t('nav.study')}</span>
          </NavLink>

          <NavLink
            to="/practical"
            className={({ isActive }) =>
              `flex min-h-11 items-center gap-3 rounded-lg px-3 text-sm font-medium transition-colors ${
                isActive
                  ? 'bg-accent-soft text-accent'
                  : 'text-ink-muted hover:bg-subtle hover:text-ink'
              }`
            }
          >
            <Timer className="h-4 w-4 shrink-0" />
            <span>{t('nav.practical')}</span>
          </NavLink>

          <div className="my-2 border-t border-border" />

          <button
            type="button"
            disabled
            aria-disabled="true"
            className="flex min-h-11 cursor-not-allowed items-center justify-between rounded-lg px-3 text-sm text-ink-muted/60"
          >
            <span className="flex items-center gap-3">
              <ClipboardCheck className="h-4 w-4 shrink-0" />
              <span>{t('nav.triage')}</span>
            </span>
            <span className="rounded bg-subtle px-1.5 py-0.5 text-[11px]">{t('app.comingSoon')}</span>
          </button>

          <button
            type="button"
            disabled
            aria-disabled="true"
            className="flex min-h-11 cursor-not-allowed items-center justify-between rounded-lg px-3 text-sm text-ink-muted/60"
          >
            <span className="flex items-center gap-3">
              <FileOutput className="h-4 w-4 shrink-0" />
              <span>{t('nav.export')}</span>
            </span>
            <span className="rounded bg-subtle px-1.5 py-0.5 text-[11px]">{t('app.comingSoon')}</span>
          </button>
        </nav>

        <div className="border-t border-border p-3 space-y-1.5">
          {mockActive && (
            <div className="rounded-md bg-warn-soft px-2.5 py-1.5 text-xs font-medium text-warn">
              {t('app.mockActive')}
            </div>
          )}

          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={toggleLanguage}
              className="flex min-h-11 flex-1 items-center justify-center gap-1.5 rounded-lg border border-border bg-canvas px-2.5 text-xs font-medium text-ink hover:bg-subtle"
              title={t('app.language')}
            >
              <Globe className="h-3.5 w-3.5" />
              <span>{i18n.language === 'zh-TW' ? '繁中' : 'EN'}</span>
            </button>

            <button
              type="button"
              onClick={cycleTheme}
              className="flex min-h-11 flex-1 items-center justify-center gap-1.5 rounded-lg border border-border bg-canvas px-2.5 text-xs font-medium text-ink hover:bg-subtle"
              title={t('app.theme')}
            >
              <ThemeIcon className="h-3.5 w-3.5" />
              <span>{themeLabel}</span>
            </button>
          </div>

          {authStatus?.auth_mode === 'invite' && (
            authStatus.authenticated ? (
              <button
                type="button"
                onClick={() => logout.mutate()}
                className="flex min-h-11 w-full items-center justify-center gap-2 rounded-lg border border-border px-3 text-xs font-medium text-ink-muted hover:bg-subtle hover:text-ink"
              >
                <LogOut className="h-3.5 w-3.5" />
                <span>{t('nav.signOut')}</span>
              </button>
            ) : (
              <Link
                to="/login"
                className="flex min-h-11 w-full items-center justify-center gap-2 rounded-lg bg-accent px-3 text-xs font-medium text-white hover:bg-accent-hover"
              >
                <LogIn className="h-3.5 w-3.5" />
                <span>{t('nav.signIn')}</span>
              </Link>
            )
          )}
        </div>
      </aside>

      {/* Main Column */}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* Mobile / iPad Portrait Top Bar */}
        <header className="sticky top-0 z-30 flex items-center justify-between border-b border-border bg-surface/95 px-4 py-2 backdrop-blur lg:hidden">
          <Link to="/" className="flex min-h-11 items-center gap-2">
            <img src="/favicon.svg" alt="" className="h-6 w-6 rounded-md" />
            <span className="text-base font-semibold tracking-tight text-ink">{t('app.name')}</span>
          </Link>

          <div className="flex items-center gap-1">
            {mockActive && (
              <span className="mr-1 rounded bg-warn-soft px-2 py-0.5 text-[11px] font-medium text-warn">
                Mock
              </span>
            )}
            <button
              type="button"
              onClick={toggleLanguage}
              className="flex min-h-11 min-w-11 items-center justify-center rounded-lg px-2 text-xs font-medium text-ink-muted hover:bg-subtle hover:text-ink"
              aria-label={t('app.language')}
            >
              {i18n.language === 'zh-TW' ? '繁中' : 'EN'}
            </button>
            <button
              type="button"
              onClick={cycleTheme}
              className="flex min-h-11 min-w-11 items-center justify-center rounded-lg text-ink-muted hover:bg-subtle hover:text-ink"
              aria-label={t('app.theme')}
            >
              <ThemeIcon className="h-4 w-4" />
            </button>
          </div>
        </header>

        {needsSignIn && location.pathname !== '/login' && (
          <div className="border-b border-border bg-accent-soft px-4 py-3 text-sm text-ink">
            <div className="mx-auto flex max-w-5xl items-center justify-between gap-4">
              <span>{t('auth.subtitleInvite')}</span>
              <Link
                to="/login"
                className="inline-flex min-h-11 shrink-0 items-center rounded-lg bg-accent px-4 text-sm font-medium text-white hover:bg-accent-hover"
              >
                {t('nav.signIn')}
              </Link>
            </div>
          </div>
        )}

        <main className="flex-1 pb-20 lg:pb-8">
          <Outlet />
        </main>
      </div>

      {/* Mobile / iPad Portrait Bottom Navigation */}
      <nav
        aria-label="Mobile navigation"
        className="pb-safe fixed inset-x-0 bottom-0 z-30 border-t border-border bg-surface/95 backdrop-blur lg:hidden"
      >
        <div className="mx-auto grid max-w-lg grid-cols-3 gap-1 px-2 pt-1.5">
          <NavLink
            to="/"
            end
            className={({ isActive }) =>
              `flex min-h-12 flex-col items-center justify-center gap-0.5 rounded-lg text-xs font-medium ${
                isActive ? 'text-accent' : 'text-ink-muted'
              }`
            }
          >
            <BookOpen className="h-5 w-5" />
            <span>{t('nav.reading')}</span>
          </NavLink>

          <NavLink
            to="/study"
            className={({ isActive }) =>
              `flex min-h-12 flex-col items-center justify-center gap-0.5 rounded-lg text-xs font-medium ${
                isActive ? 'text-accent' : 'text-ink-muted'
              }`
            }
          >
            <GraduationCap className="h-5 w-5" />
            <span>{t('nav.study')}</span>
          </NavLink>

          <NavLink
            to="/practical"
            className={({ isActive }) =>
              `flex min-h-12 flex-col items-center justify-center gap-0.5 rounded-lg text-xs font-medium ${
                isActive ? 'text-accent' : 'text-ink-muted'
              }`
            }
          >
            <Timer className="h-5 w-5" />
            <span>{t('nav.practical')}</span>
          </NavLink>
        </div>
      </nav>
    </div>
  )
}
