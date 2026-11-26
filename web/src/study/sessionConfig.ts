import type { SessionMode } from '../api/types'

export interface StoredSessionConfig {
  mode: SessionMode
  secondsPerQuestion: number | null
  totalTimeSecs: number | null
}

const PREFIX = 'reinforce:session-cfg:'

export function saveSessionConfig(sessionId: string, config: StoredSessionConfig): void {
  try {
    window.sessionStorage.setItem(`${PREFIX}${sessionId}`, JSON.stringify(config))
  } catch {
    // Ignore storage quota errors in restricted browsing modes.
  }
}

export function loadSessionConfig(sessionId: string): StoredSessionConfig | null {
  try {
    const raw = window.sessionStorage.getItem(`${PREFIX}${sessionId}`)
    if (!raw) return null
    return JSON.parse(raw) as StoredSessionConfig
  } catch {
    return null
  }
}
