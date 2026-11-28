import type { AuthStatus, PluginJob, PluginJobStatus } from '../api/types.ts'

/** Placeholder shown wherever a real agent token would go in a config sample. */
export const AGENT_TOKEN_PLACEHOLDER = '<agent-token>'

/** Poll interval while at least one plugin job can still change state. */
export const ACTIVE_JOB_POLL_MS = 2_000

export interface SettingsAccess {
  /** The caller may use owner-only surfaces such as plugins. */
  isOwner: boolean
  /** Named agent tokens exist only in invite mode. */
  isInviteMode: boolean
  /** Owner in invite mode: may create, list and revoke agent tokens. */
  canManageTokens: boolean
}

/**
 * Derives what the settings page may show from `/api/auth/status`. Single mode
 * has no login wall and the server treats every caller as the owner.
 */
export function deriveSettingsAccess(status: AuthStatus | undefined): SettingsAccess {
  if (!status) return { isOwner: false, isInviteMode: false, canManageTokens: false }
  const isInviteMode = status.auth_mode === 'invite'
  const isOwner = isInviteMode
    ? status.authenticated && status.role === 'owner'
    : status.role === 'owner' || status.role === null
  return { isOwner, isInviteMode, canManageTokens: isInviteMode && isOwner }
}

/** Local stdio client configuration for `reinforce mcp`. */
export function buildStdioConfig(): string {
  return JSON.stringify(
    {
      mcpServers: {
        reinforce: {
          command: '/usr/local/bin/reinforce',
          args: [
            'mcp',
            '--db',
            '/path/to/reinforce.db',
            '--blobs',
            '/path/to/blobs',
            '--agent-name',
            'claude',
          ],
        },
      },
    },
    null,
    2,
  )
}

/**
 * Remote Streamable HTTP configuration. Invite mode authorizes each call with a
 * bearer agent token; single mode accepts loopback callers without one.
 */
export function buildHttpConfig(origin: string, inviteMode: boolean, token?: string): string {
  const url = `${origin.replace(/\/+$/, '')}/mcp`
  const server: { url: string; headers?: Record<string, string> } = { url }
  if (inviteMode) {
    server.headers = { Authorization: `Bearer ${token || AGENT_TOKEN_PLACEHOLDER}` }
  }
  return JSON.stringify({ mcpServers: { reinforce: server } }, null, 2)
}

export type JobInputResult =
  | { ok: true; value: Record<string, unknown> }
  | { ok: false; reason: 'empty' | 'invalid' | 'notObject'; message?: string }

/** Validates the job payload the same way the server does: a JSON object. */
export function parseJobInput(text: string): JobInputResult {
  const trimmed = text.trim()
  if (!trimmed) return { ok: false, reason: 'empty' }
  let parsed: unknown
  try {
    parsed = JSON.parse(trimmed)
  } catch (error) {
    return {
      ok: false,
      reason: 'invalid',
      message: error instanceof Error ? error.message : String(error),
    }
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    return { ok: false, reason: 'notObject' }
  }
  return { ok: true, value: parsed as Record<string, unknown> }
}

export function isActiveJobStatus(status: PluginJobStatus): boolean {
  return status === 'queued' || status === 'running'
}

/** A job can be cancelled while it is still active and no cancel is pending. */
export function canCancelJob(job: PluginJob): boolean {
  return isActiveJobStatus(job.status) && !job.cancel_requested
}

export function hasActiveJobs(jobs: readonly PluginJob[] | undefined): boolean {
  return Boolean(jobs?.some((job) => isActiveJobStatus(job.status)))
}

/**
 * The server stores progress as a 0–1 fraction. Returns a whole percentage
 * clamped to 0–100; non-finite input reads as 0.
 */
export function progressPercent(progress: number): number {
  if (!Number.isFinite(progress)) return 0
  return Math.round(Math.min(1, Math.max(0, progress)) * 100)
}

/** Newest first, so a just-submitted job lands at the top of the queue. */
export function sortJobsNewestFirst(jobs: readonly PluginJob[]): PluginJob[] {
  return [...jobs].sort((a, b) => b.created_at.localeCompare(a.created_at))
}
