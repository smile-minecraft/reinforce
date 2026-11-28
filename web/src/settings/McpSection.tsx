import { Info, KeyRound, Plug, RefreshCw, Terminal, Trash2, TriangleAlert, X } from 'lucide-react'
import { useId, useMemo, useState, type SubmitEvent } from 'react'
import { useTranslation } from 'react-i18next'
import {
  useAgentTokens,
  useCreateAgentToken,
  useRevokeAgentToken,
} from '../api/hooks/useAgentTokens'
import type { CreatedAgentToken } from '../api/types'
import { formatShortTimestamp } from '../triage/proposalHelpers'
import { CopyButton } from './CopyButton'
import { buildHttpConfig, buildStdioConfig, type SettingsAccess } from './settingsLogic'

function errorText(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback
}

function ConfigSample({ title, hint, value }: { title: string; hint: string; value: string }) {
  const { t } = useTranslation()
  const headingId = useId()
  return (
    <div className="space-y-2" role="group" aria-labelledby={headingId}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 id={headingId} className="text-xs font-semibold text-ink">
            {title}
          </h3>
          <p className="mt-0.5 text-xs text-ink-muted">{hint}</p>
        </div>
        <CopyButton value={value} label={t('settings.copy.copyNamed', { name: title })} />
      </div>
      <pre className="max-h-64 overflow-auto rounded-xl border border-border bg-surface p-3 font-mono text-[11px] leading-relaxed text-ink">
        <code>{value}</code>
      </pre>
    </div>
  )
}

export function McpSection({ access }: { access: SettingsAccess }) {
  const { t } = useTranslation()
  const origin = typeof window === 'undefined' ? '' : window.location.origin
  const stdioConfig = useMemo(() => buildStdioConfig(), [])
  const httpConfig = useMemo(
    () => buildHttpConfig(origin, access.isInviteMode),
    [origin, access.isInviteMode],
  )

  return (
    <section
      aria-labelledby="settings-mcp-heading"
      className="space-y-4 rounded-2xl border border-border bg-canvas/40 p-5"
    >
      <header>
        <h2
          id="settings-mcp-heading"
          className="flex items-center gap-2 text-sm font-semibold text-ink"
        >
          <Plug className="h-4 w-4 text-accent" />
          <span>{t('settings.mcp.title')}</span>
        </h2>
        <p className="mt-1 text-xs text-ink-muted">{t('settings.mcp.subtitle')}</p>
      </header>

      <ConfigSample
        title={t('settings.mcp.stdioTitle')}
        hint={t('settings.mcp.stdioHint')}
        value={stdioConfig}
      />
      <ConfigSample
        title={t('settings.mcp.httpTitle')}
        hint={
          access.isInviteMode ? t('settings.mcp.httpHintInvite') : t('settings.mcp.httpHintSingle')
        }
        value={httpConfig}
      />

      {access.canManageTokens ? (
        <AgentTokenManager origin={origin} />
      ) : (
        <p className="flex items-start gap-2 rounded-xl border border-border bg-surface px-3 py-2.5 text-xs text-ink-muted">
          <Info className="mt-0.5 h-3.5 w-3.5 shrink-0 text-accent" />
          <span>
            {access.isInviteMode
              ? t('settings.mcp.tokensOwnerOnly')
              : t('settings.mcp.tokensSingleMode')}
          </span>
        </p>
      )}
    </section>
  )
}

function AgentTokenManager({ origin }: { origin: string }) {
  const { t } = useTranslation()
  const nameInputId = useId()
  const tokens = useAgentTokens(true)
  const create = useCreateAgentToken()
  const revoke = useRevokeAgentToken()
  const [name, setName] = useState('')
  const [created, setCreated] = useState<CreatedAgentToken | null>(null)
  const [confirmingId, setConfirmingId] = useState<string | null>(null)
  const [revokeNotice, setRevokeNotice] = useState<string | null>(null)

  function handleCreate(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault()
    const trimmed = name.trim()
    setRevokeNotice(null)
    create.mutate(trimmed ? { name: trimmed } : {}, {
      onSuccess: (result) => {
        setCreated(result)
        setName('')
      },
    })
  }

  function handleRevoke(id: string) {
    setRevokeNotice(null)
    revoke.mutate(id, {
      onSuccess: (result) => {
        setConfirmingId(null)
        if (!result.revoked) setRevokeNotice(t('settings.tokens.alreadyRevoked'))
        if (created?.id === id) setCreated(null)
      },
    })
  }

  const list = tokens.data?.tokens ?? []

  return (
    <div className="space-y-4 border-t border-border pt-4">
      <div>
        <h3 className="flex items-center gap-2 text-xs font-semibold text-ink">
          <KeyRound className="h-3.5 w-3.5 text-accent" />
          <span>{t('settings.tokens.title')}</span>
        </h3>
        <p className="mt-0.5 text-xs text-ink-muted">{t('settings.tokens.subtitle')}</p>
      </div>

      <form onSubmit={handleCreate} className="flex flex-col gap-2 sm:flex-row">
        <label htmlFor={nameInputId} className="sr-only">
          {t('settings.tokens.nameLabel')}
        </label>
        <input
          id={nameInputId}
          type="text"
          value={name}
          maxLength={64}
          onChange={(event) => setName(event.target.value)}
          placeholder={t('settings.tokens.namePlaceholder')}
          className="h-10 min-w-0 flex-1 rounded-xl border border-border bg-surface px-3 text-sm text-ink"
        />
        <button
          type="submit"
          disabled={create.isPending}
          className="inline-flex h-10 items-center justify-center gap-2 rounded-xl bg-accent px-4 text-xs font-semibold text-white hover:bg-accent-hover disabled:opacity-60"
        >
          <KeyRound className="h-3.5 w-3.5" />
          <span>{create.isPending ? t('settings.tokens.creating') : t('settings.tokens.create')}</span>
        </button>
      </form>

      {create.isError && (
        <p role="alert" className="rounded-xl border border-bad/40 bg-bad-soft px-3 py-2 text-xs text-bad">
          {t('settings.tokens.createFailed', { message: errorText(create.error, t('common.errorGeneric')) })}
        </p>
      )}

      {created && (
        <div
          role="status"
          className="space-y-2 rounded-xl border border-warn/40 bg-warn-soft p-3 text-xs text-ink"
        >
          <div className="flex items-start justify-between gap-2">
            <p className="flex items-start gap-2 font-medium">
              <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0 text-warn" />
              <span>
                {t('settings.tokens.createdNotice', {
                  name: created.name ?? t('settings.tokens.unnamed'),
                })}
              </span>
            </p>
            <button
              type="button"
              onClick={() => setCreated(null)}
              aria-label={t('settings.tokens.dismiss')}
              className="rounded-md p-1 text-ink-muted hover:bg-subtle hover:text-ink"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
            <input
              readOnly
              value={created.token}
              aria-label={t('settings.tokens.plaintextLabel')}
              onFocus={(event) => event.currentTarget.select()}
              className="h-9 min-w-0 flex-1 rounded-lg border border-border bg-surface px-2.5 font-mono text-[11px] text-ink"
            />
            <div className="flex gap-2">
              <CopyButton value={created.token} label={t('settings.tokens.copyToken')} variant="solid" />
              <CopyButton
                value={buildHttpConfig(origin, true, created.token)}
                label={t('settings.tokens.copyConfig')}
              />
            </div>
          </div>
        </div>
      )}

      {revokeNotice && (
        <p role="status" className="text-xs text-ink-muted">
          {revokeNotice}
        </p>
      )}
      {revoke.isError && (
        <p role="alert" className="rounded-xl border border-bad/40 bg-bad-soft px-3 py-2 text-xs text-bad">
          {t('settings.tokens.revokeFailed', { message: errorText(revoke.error, t('common.errorGeneric')) })}
        </p>
      )}

      {tokens.isLoading ? (
        <p className="text-xs text-ink-muted">{t('common.loading')}</p>
      ) : tokens.isError ? (
        <div role="alert" className="flex items-center justify-between gap-2 rounded-xl border border-bad/40 bg-bad-soft px-3 py-2 text-xs text-bad">
          <span>{t('settings.tokens.loadFailed', { message: errorText(tokens.error, t('common.errorGeneric')) })}</span>
          <button
            type="button"
            onClick={() => void tokens.refetch()}
            className="inline-flex items-center gap-1 font-medium underline"
          >
            <RefreshCw className="h-3 w-3" />
            {t('common.retry')}
          </button>
        </div>
      ) : list.length === 0 ? (
        <p className="text-xs text-ink-muted">{t('settings.tokens.empty')}</p>
      ) : (
        <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface" aria-label={t('settings.tokens.listLabel')}>
          {list.map((token) => {
            const label = token.name ?? t('settings.tokens.unnamed')
            const confirming = confirmingId === token.id
            const pending = revoke.isPending && revoke.variables === token.id
            return (
              <li key={token.id} className="flex flex-wrap items-center justify-between gap-2 px-3 py-2.5">
                <div className="min-w-0">
                  <p className="flex items-center gap-1.5 truncate text-sm font-medium text-ink">
                    <Terminal className="h-3.5 w-3.5 shrink-0 text-ink-muted" />
                    <span className={token.name ? '' : 'italic text-ink-muted'}>{label}</span>
                  </p>
                  <p className="mt-0.5 text-[11px] text-ink-muted">
                    {t('settings.tokens.createdAt', { time: formatShortTimestamp(token.created_at) })}
                  </p>
                </div>
                {confirming ? (
                  <div className="flex items-center gap-2" role="group" aria-label={t('settings.tokens.confirmRevoke', { name: label })}>
                    <span className="text-xs text-bad">{t('settings.tokens.confirmRevokeShort')}</span>
                    <button
                      type="button"
                      onClick={() => handleRevoke(token.id)}
                      disabled={pending}
                      className="h-8 rounded-lg bg-bad px-3 text-xs font-semibold text-white disabled:opacity-60"
                    >
                      {pending ? t('settings.tokens.revoking') : t('settings.tokens.revoke')}
                    </button>
                    <button
                      type="button"
                      onClick={() => setConfirmingId(null)}
                      disabled={pending}
                      className="h-8 rounded-lg border border-border px-3 text-xs text-ink-muted hover:text-ink"
                    >
                      {t('common.cancel')}
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={() => {
                      revoke.reset()
                      setConfirmingId(token.id)
                    }}
                    aria-label={t('settings.tokens.revokeNamed', { name: label })}
                    className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-border px-2.5 text-xs text-ink-muted hover:border-bad/50 hover:text-bad"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                    <span>{t('settings.tokens.revoke')}</span>
                  </button>
                )}
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}
