import { useQuery } from '@tanstack/react-query'
import { api } from '../client'

export const noteKeys = {
  all: ['notes'] as const,
  list: (params?: { subject?: string; status?: string; limit?: number }) =>
    ['notes', 'list', params ?? {}] as const,
  detail: (id: string) => ['notes', 'detail', id] as const,
}

export function useNotes(params?: { subject?: string; status?: string; limit?: number }) {
  return useQuery({
    queryKey: noteKeys.list(params),
    queryFn: () => api.listNotes(params),
  })
}

export function useNote(id: string | undefined) {
  return useQuery({
    queryKey: noteKeys.detail(id ?? ''),
    queryFn: () => api.getNote(id!),
    enabled: Boolean(id),
  })
}
