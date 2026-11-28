import { useQueries, useQuery } from '@tanstack/react-query'
import { api } from '../client'

export const cardKeys = {
  all: ['knowledge_cards'] as const,
  list: (params?: { subject?: string; status?: string; limit?: number }) =>
    ['knowledge_cards', 'list', params ?? {}] as const,
  detail: (id: string) => ['knowledge_cards', 'detail', id] as const,
  questions: (id: string) => ['knowledge_cards', 'questions', id] as const,
  neighbors: (kind: 'card' | 'question' | 'option', id: string, depth: number) =>
    ['graph', kind, id, depth] as const,
}

export function useKnowledgeCards(params?: { subject?: string; status?: string; limit?: number }) {
  return useQuery({
    queryKey: cardKeys.list(params),
    queryFn: () => api.listKnowledgeCards(params),
  })
}

export function useKnowledgeCard(id: string | undefined) {
  return useQuery({
    queryKey: cardKeys.detail(id ?? ''),
    queryFn: () => api.getKnowledgeCard(id!),
    enabled: Boolean(id),
  })
}

export function useKnowledgeCardsByIds(ids: string[]) {
  const uniqueIds = Array.from(new Set(ids))
  return useQueries({
    queries: uniqueIds.map((id) => ({
      queryKey: cardKeys.detail(id),
      queryFn: () => api.getKnowledgeCard(id),
      staleTime: 60_000,
    })),
  })
}

export function useGraphNeighbors(
  kind: 'card' | 'question' | 'option',
  id: string | undefined,
  depth = 1,
) {
  return useQuery({
    queryKey: cardKeys.neighbors(kind, id ?? '', depth),
    queryFn: () => api.graphNeighbors(kind, id!, depth),
    enabled: Boolean(id),
  })
}

export function useCardQuestions(id: string | undefined) {
  return useQuery({
    queryKey: cardKeys.questions(id ?? ''),
    queryFn: async () => {
      try {
        return await api.listCardQuestions(id!)
      } catch {
        return { question_ids: [], count: 0 }
      }
    },
    enabled: Boolean(id),
    staleTime: 60_000,
  })
}
