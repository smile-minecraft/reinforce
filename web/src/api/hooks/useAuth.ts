import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../client'

export const authKeys = {
  meta: ['meta'] as const,
  status: ['auth', 'status'] as const,
  session: ['auth', 'session'] as const,
}

export function useMeta() {
  return useQuery({
    queryKey: authKeys.meta,
    queryFn: api.getMeta,
    staleTime: 5 * 60_000,
  })
}

export function useAuthStatus() {
  return useQuery({
    queryKey: authKeys.status,
    queryFn: api.getAuthStatus,
    staleTime: 60_000,
  })
}

export function useAuthSession(enabled = true) {
  return useQuery({
    queryKey: authKeys.session,
    queryFn: api.getAuthSession,
    enabled,
    retry: false,
  })
}

export function useLogin() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: api.login,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['auth'] })
      void qc.invalidateQueries({ queryKey: ['notes'] })
      void qc.invalidateQueries({ queryKey: ['knowledge_cards'] })
    },
  })
}

export function useRegister() {
  return useMutation({
    mutationFn: api.register,
  })
}

export function useLogout() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: api.logout,
    onSuccess: () => {
      qc.clear()
      void qc.invalidateQueries({ queryKey: authKeys.status })
    },
  })
}
