import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../client'
import type { UpdateNoteViewSettingsRequest } from '../types'

export const noteKeys = {
  all: ['notes'] as const,
  list: (params?: { subject?: string; status?: string; limit?: number }) =>
    ['notes', 'list', params ?? {}] as const,
  detail: (id: string) => ['notes', 'detail', id] as const,
  edges: (id: string) => ['notes', 'edges', id] as const,
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

export function useNoteEdges(id: string | undefined) {
  return useQuery({
    queryKey: noteKeys.edges(id ?? ''),
    queryFn: () => api.listNoteEdges(id!),
    enabled: Boolean(id),
  })
}

export function useUpdateNoteViewSettings(id: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: UpdateNoteViewSettingsRequest) => {
      if (!id) throw new Error('Note id is required')
      return api.updateNoteViewSettings(id, body)
    },
    onSuccess: (updatedNote) => {
      qc.setQueryData(noteKeys.detail(updatedNote.id), updatedNote)
      void qc.invalidateQueries({ queryKey: noteKeys.all })
    },
  })
}
