import { useMutation } from '@tanstack/react-query'
import { api } from '../client'
import type { ExportRequest } from '../types'

export function useExportContent() {
  return useMutation({
    mutationFn: (request: ExportRequest) => api.exportContent(request),
  })
}
