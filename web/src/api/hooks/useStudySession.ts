import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../client'
import type { AttemptDirection, CreateSessionRequest, QuizAnswer } from '../types'

export const studyKeys = {
  session: (id: string) => ['study', 'session', id] as const,
  score: (id: string) => ['study', 'score', id] as const,
  attempts: (params?: { card_id?: string; direction?: AttemptDirection }) =>
    ['study', 'attempts', params ?? {}] as const,
  scheduling: (cardId: string, direction?: AttemptDirection) =>
    ['study', 'scheduling', cardId, direction ?? 'default'] as const,
  weakness: (limit: number) => ['study', 'weakness', limit] as const,
}

export function useCreateSession() {
  return useMutation({
    mutationFn: (request: CreateSessionRequest) => api.createSession(request),
  })
}

export function useStudySession(sessionId: string | undefined) {
  return useQuery({
    queryKey: studyKeys.session(sessionId ?? ''),
    queryFn: () => api.getSession(sessionId!),
    enabled: Boolean(sessionId),
    refetchOnWindowFocus: false,
  })
}

export function usePauseSession(sessionId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => api.pauseSession(sessionId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: studyKeys.session(sessionId) })
    },
  })
}

export function useResumeSession(sessionId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => api.resumeSession(sessionId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: studyKeys.session(sessionId) })
    },
  })
}

export function useTickSession(sessionId: string) {
  return useMutation({
    mutationFn: () => api.tickSession(sessionId),
  })
}

export function useSubmitAnswer(sessionId: string) {
  return useMutation({
    mutationFn: (answer: QuizAnswer) => api.submitAnswer(sessionId, answer),
  })
}

export function useFinishSession(sessionId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => api.finishSession(sessionId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: studyKeys.session(sessionId) })
      void qc.invalidateQueries({ queryKey: studyKeys.score(sessionId) })
      void qc.invalidateQueries({ queryKey: ['study', 'weakness'] })
      void qc.invalidateQueries({ queryKey: ['study', 'attempts'] })
    },
  })
}

export function useSessionScore(sessionId: string | undefined, enabled = true) {
  return useQuery({
    queryKey: studyKeys.score(sessionId ?? ''),
    queryFn: () => api.getSessionScore(sessionId!),
    enabled: Boolean(sessionId) && enabled,
  })
}

export function useAttempts(params?: { card_id?: string; direction?: AttemptDirection }) {
  return useQuery({
    queryKey: studyKeys.attempts(params),
    queryFn: () => api.listAttempts(params),
  })
}

export function useScheduling(cardId: string | undefined, direction?: AttemptDirection) {
  return useQuery({
    queryKey: studyKeys.scheduling(cardId ?? '', direction),
    queryFn: () => api.getScheduling(cardId!, direction),
    enabled: Boolean(cardId),
  })
}

export function useWeaknessReport(limit = 20) {
  return useQuery({
    queryKey: studyKeys.weakness(limit),
    queryFn: () => api.getWeaknessReport(limit),
  })
}
