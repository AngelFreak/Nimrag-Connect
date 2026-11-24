import { useQuery } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { RecordsResponse } from '../../api/generated';

export function useRecords() {
  return useQuery({
    queryKey: ['records'],
    queryFn: ({ signal }) => api.get<RecordsResponse>('/api/records', { signal }),
  });
}
