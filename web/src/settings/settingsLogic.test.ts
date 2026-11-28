import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import type { PluginJob } from '../api/types.ts'
import {
  AGENT_TOKEN_PLACEHOLDER,
  buildHttpConfig,
  buildStdioConfig,
  canCancelJob,
  deriveSettingsAccess,
  hasActiveJobs,
  parseJobInput,
  progressPercent,
  sortJobsNewestFirst,
} from './settingsLogic.ts'

function makeJob(overrides: Partial<PluginJob> = {}): PluginJob {
  return {
    id: 'job-1',
    plugin: 'mineru',
    capability: 'ingest_pdf',
    input: {},
    status: 'queued',
    attempts: 0,
    max_attempts: 3,
    cancel_requested: false,
    progress: 0,
    progress_message: null,
    proposal_id: null,
    error: null,
    created_at: '2026-11-28T10:00:00Z',
    updated_at: '2026-11-28T10:00:00Z',
    started_at: null,
    finished_at: null,
    ...overrides,
  }
}

describe('deriveSettingsAccess', () => {
  it('denies everything while status is unknown', () => {
    assert.deepEqual(deriveSettingsAccess(undefined), {
      isOwner: false,
      isInviteMode: false,
      canManageTokens: false,
    })
  })

  it('treats single mode as owner without token management', () => {
    const access = deriveSettingsAccess({
      auth_mode: 'single',
      needs_owner_setup: false,
      authenticated: true,
      role: 'owner',
    })
    assert.deepEqual(access, { isOwner: true, isInviteMode: false, canManageTokens: false })
  })

  it('lets only an authenticated invite-mode owner manage tokens', () => {
    const base = { auth_mode: 'invite' as const, needs_owner_setup: false }
    assert.equal(
      deriveSettingsAccess({ ...base, authenticated: true, role: 'owner' }).canManageTokens,
      true,
    )
    const learner = deriveSettingsAccess({ ...base, authenticated: true, role: 'learner' })
    assert.equal(learner.canManageTokens, false)
    assert.equal(learner.isOwner, false)
    const anonymous = deriveSettingsAccess({ ...base, authenticated: false, role: null })
    assert.equal(anonymous.isOwner, false)
    assert.equal(anonymous.canManageTokens, false)
  })
})

describe('MCP config samples', () => {
  it('stdio config runs the mcp subcommand', () => {
    const parsed = JSON.parse(buildStdioConfig())
    assert.equal(parsed.mcpServers.reinforce.args[0], 'mcp')
  })

  it('http config adds a bearer header only in invite mode', () => {
    const single = JSON.parse(buildHttpConfig('http://127.0.0.1:3000/', false))
    assert.deepEqual(single.mcpServers.reinforce, { url: 'http://127.0.0.1:3000/mcp' })

    const invite = JSON.parse(buildHttpConfig('https://r.example.org', true))
    assert.equal(
      invite.mcpServers.reinforce.headers.Authorization,
      `Bearer ${AGENT_TOKEN_PLACEHOLDER}`,
    )
    const withToken = JSON.parse(buildHttpConfig('https://r.example.org', true, 'abc'))
    assert.equal(withToken.mcpServers.reinforce.headers.Authorization, 'Bearer abc')
  })
})

describe('parseJobInput', () => {
  it('accepts a JSON object', () => {
    assert.deepEqual(parseJobInput(' {"resource_id": "r1"} '), {
      ok: true,
      value: { resource_id: 'r1' },
    })
  })

  it('rejects empty, malformed and non-object payloads', () => {
    assert.deepEqual(parseJobInput('   '), { ok: false, reason: 'empty' })
    const invalid = parseJobInput('{nope')
    assert.equal(invalid.ok, false)
    assert.equal(!invalid.ok && invalid.reason, 'invalid')
    assert.deepEqual(parseJobInput('[1, 2]'), { ok: false, reason: 'notObject' })
    assert.deepEqual(parseJobInput('null'), { ok: false, reason: 'notObject' })
    assert.deepEqual(parseJobInput('"text"'), { ok: false, reason: 'notObject' })
  })
})

describe('job helpers', () => {
  it('only active jobs without a pending cancel can be cancelled', () => {
    assert.equal(canCancelJob(makeJob({ status: 'queued' })), true)
    assert.equal(canCancelJob(makeJob({ status: 'running' })), true)
    assert.equal(canCancelJob(makeJob({ status: 'running', cancel_requested: true })), false)
    assert.equal(canCancelJob(makeJob({ status: 'succeeded' })), false)
    assert.equal(canCancelJob(makeJob({ status: 'failed' })), false)
    assert.equal(canCancelJob(makeJob({ status: 'cancelled' })), false)
  })

  it('polls only while some job is queued or running', () => {
    assert.equal(hasActiveJobs(undefined), false)
    assert.equal(hasActiveJobs([makeJob({ status: 'succeeded' })]), false)
    assert.equal(
      hasActiveJobs([makeJob({ status: 'failed' }), makeJob({ status: 'running' })]),
      true,
    )
  })

  it('clamps progress fractions to a whole percentage', () => {
    assert.equal(progressPercent(0.256), 26)
    assert.equal(progressPercent(-1), 0)
    assert.equal(progressPercent(4), 100)
    assert.equal(progressPercent(Number.NaN), 0)
  })

  it('sorts newest first without mutating the input', () => {
    const older = makeJob({ id: 'a', created_at: '2026-11-28T09:00:00Z' })
    const newer = makeJob({ id: 'b', created_at: '2026-11-28T11:00:00Z' })
    const input = [older, newer]
    assert.deepEqual(
      sortJobsNewestFirst(input).map((job) => job.id),
      ['b', 'a'],
    )
    assert.deepEqual(
      input.map((job) => job.id),
      ['a', 'b'],
    )
  })
})
