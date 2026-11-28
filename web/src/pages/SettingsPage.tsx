import { Globe, LogIn, LogOut, Monitor, Moon, Settings, Sun, User } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { useAuthStatus, useLogout, useMeta } from '../api/hooks/useAuth'
import { useTheme, type ThemePreference } from '../components/themeContext'
import { SUPPORTED_LANGUAGES, type Language } from '../i18n'
import { McpSection } from '../settings/McpSection'
import { PluginsSection } from '../settings/PluginsSection'
import { deriveSettingsAccess } from '../settings/settingsLogic'
import { useWorkspace } from '../workspace/workspaceContextValue'

const THEME_OPTIONS: Array<{ id: ThemePreference; icon: typeof Sun; key: string }> = [
  { id: 'system', icon: Monitor, key: 'app.themeSystem' },
  { id: 'light', icon: Sun, key: 'app.themeLight' },
  { id: 'dark', icon: Moon, key: 'app.themeDark' },
]

export function SettingsPage() {
  const { t, i18n } = useTranslation()
  const { preference, setPreference } = useTheme()
  const { displayName, setDisplayName } = useWorkspace()
  const { data: authStatus } = useAuthStatus()
  const { data: meta } = useMeta()
  const logout = useLogout()
  const access = deriveSettingsAccess(authStatus)

  const [nameInput, setNameInput] = useState(displayName)

  function handleSaveName() {
    setDisplayName(nameInput)
  }

  return (
    <div className="rf-pane-scroll flex-1 overflow-y-auto px-5 py-6 sm:px-8">
      <div className="mx-auto max-w-2xl space-y-6">
        <header>
          <h1 className="flex items-center gap-2 text-2xl font-bold tracking-tight text-ink">
            <Settings className="h-5 w-5 text-accent" />
            <span>{t('workspace.settings.title')}</span>
          </h1>
          <p className="mt-1 text-sm text-ink-muted">{t('workspace.settings.subtitle')}</p>
        </header>

        {/* Display Name */}
        <section className="space-y-3 rounded-2xl border border-border bg-canvas/40 p-5">
          <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
            <User className="h-4 w-4 text-accent" />
            <span>{t('workspace.settings.displayName')}</span>
          </h2>
          <div className="flex gap-2">
            <input
              type="text"
              value={nameInput}
              onChange={(e) => setNameInput(e.target.value)}
              placeholder={t('workspace.settings.displayNamePlaceholder')}
              className="h-10 flex-1 rounded-xl border border-border bg-surface px-3 text-sm text-ink"
            />
            <button
              type="button"
              onClick={handleSaveName}
              className="h-10 rounded-xl bg-accent px-4 text-xs font-semibold text-white hover:bg-accent-hover"
            >
              {t('common.confirm')}
            </button>
          </div>
        </section>

        {/* Appearance */}
        <section className="space-y-3 rounded-2xl border border-border bg-canvas/40 p-5">
          <h2 className="text-sm font-semibold text-ink">{t('workspace.settings.appearance')}</h2>
          <div className="grid grid-cols-3 gap-2">
            {THEME_OPTIONS.map(({ id, icon: Icon, key }) => {
              const active = preference === id
              return (
                <button
                  key={id}
                  type="button"
                  onClick={() => setPreference(id)}
                  className={`flex h-11 items-center justify-center gap-2 rounded-xl border text-xs font-medium transition-colors ${
                    active
                      ? 'border-accent bg-accent-soft text-accent'
                      : 'border-border bg-surface text-ink-muted hover:text-ink'
                  }`}
                >
                  <Icon className="h-4 w-4" />
                  <span>{t(key)}</span>
                </button>
              )
            })}
          </div>
        </section>

        {/* Language */}
        <section className="space-y-3 rounded-2xl border border-border bg-canvas/40 p-5">
          <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
            <Globe className="h-4 w-4 text-accent" />
            <span>{t('workspace.settings.language')}</span>
          </h2>
          <div className="grid grid-cols-2 gap-2">
            {SUPPORTED_LANGUAGES.map((lang: Language) => {
              const active = i18n.language === lang
              return (
                <button
                  key={lang}
                  type="button"
                  onClick={() => void i18n.changeLanguage(lang)}
                  className={`h-11 rounded-xl border text-xs font-medium transition-colors ${
                    active
                      ? 'border-accent bg-accent-soft text-accent'
                      : 'border-border bg-surface text-ink-muted hover:text-ink'
                  }`}
                >
                  {lang === 'zh-TW' ? '繁體中文（台灣）' : 'English'}
                </button>
              )
            })}
          </div>
        </section>

        {/* Account & Version */}
        <section className="space-y-3 rounded-2xl border border-border bg-canvas/40 p-5">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-semibold text-ink">{t('workspace.settings.account')}</h2>
            {meta?.version && (
              <span className="rounded-full bg-subtle px-2.5 py-0.5 font-mono text-xs text-ink-muted">
                Reinforce v{meta.version}
              </span>
            )}
          </div>

          {authStatus?.auth_mode === 'invite' ? (
            authStatus.authenticated ? (
              <button
                type="button"
                onClick={() => logout.mutate()}
                className="inline-flex h-10 items-center gap-2 rounded-xl border border-border bg-surface px-4 text-xs font-medium text-ink hover:bg-subtle"
              >
                <LogOut className="h-3.5 w-3.5" />
                <span>{t('nav.signOut')}</span>
              </button>
            ) : (
              <Link
                to="/login"
                className="inline-flex h-10 items-center gap-2 rounded-xl bg-accent px-4 text-xs font-medium text-white hover:bg-accent-hover"
              >
                <LogIn className="h-3.5 w-3.5" />
                <span>{t('nav.signIn')}</span>
              </Link>
            )
          ) : (
            <p className="text-xs text-ink-muted">{t('settings.singleMode')}</p>
          )}
        </section>

        <McpSection access={access} />

        {access.isOwner && <PluginsSection />}
      </div>
    </div>
  )
}
