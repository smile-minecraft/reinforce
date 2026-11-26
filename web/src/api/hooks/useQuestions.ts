import { useQuery } from '@tanstack/react-query'
import { api } from '../client'

export const questionKeys = {
  list: (params?: { subject?: string; status?: string; limit?: number }) =>
    ['questions', 'list', params ?? {}] as const,
  detail: (id: string) => ['questions', 'detail', id] as const,
  examSets: (params?: { subject?: string; limit?: number }) =>
    ['exam_sets', 'list', params ?? {}] as const,
  examSet: (id: string) => ['exam_sets', 'detail', id] as const,
  search: (q: string, limit?: number) => ['search', q, limit ?? 50] as const,
}

export function useQuestions(params?: { subject?: string; status?: string; limit?: number }) {
  return useQuery({
    queryKey: questionKeys.list(params),
    queryFn: () => api.listQuestions(params),
  })
}

export function useQuestion(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: questionKeys.detail(id ?? ''),
    queryFn: () => api.getQuestion(id!),
    enabled: Boolean(id) && enabled,
  })
}

export function useExamSets(params?: { subject?: string; limit?: number }) {
  return useQuery({
    queryKey: questionKeys.examSets(params),
    queryFn: () => api.listExamSets(params),
  })
}

export function useExamSet(id: string | undefined) {
  return useQuery({
    queryKey: questionKeys.examSet(id ?? ''),
    queryFn: () => api.getExamSet(id!),
    enabled: Boolean(id),
  })
}

export function useSearch(query: string, limit = 30) {
  const trimmed = query.trim()
  return useQuery({
    queryKey: questionKeys.search(trimmed, limit),
    queryFn: () => api.search(trimmed, limit),
    enabled: trimmed.length > 0,
  })
}
