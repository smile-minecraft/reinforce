import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ACTIVE_JOB_POLL_MS, hasActiveJobs } from '../../settings/settingsLogic'
import { api } from '../client'
import type { SubmitPluginJobRequest } from '../types'

export const pluginKeys = {
  list: ['plugins'] as const,
  jobs: ['plugins', 'jobs'] as const,
}

export function usePlugins(enabled: boolean) {
  return useQuery({
    queryKey: pluginKeys.list,
    queryFn: api.listPlugins,
    enabled,
    retry: false,
  })
}

/** Polls while any job is queued or running, then stops on its own. */
export function usePluginJobs(enabled: boolean) {
  return useQuery({
    queryKey: pluginKeys.jobs,
    queryFn: () => api.listPluginJobs({ limit: 50 }),
    enabled,
    retry: false,
    refetchInterval: (query) => (hasActiveJobs(query.state.data) ? ACTIVE_JOB_POLL_MS : false),
  })
}

export function useSubmitPluginJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ plugin, body }: { plugin: string; body: SubmitPluginJobRequest }) =>
      api.submitPluginJob(plugin, body),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: pluginKeys.jobs })
    },
  })
}

export function useCancelPluginJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: api.cancelPluginJob,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: pluginKeys.jobs })
    },
  })
}
