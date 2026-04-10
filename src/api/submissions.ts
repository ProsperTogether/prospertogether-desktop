import api from './client';

export interface AgentSubmission {
  id: string;
  title: string;
  columnKey: string;
  priority: string;
  createdAt: string;
  customFields?: {
    source?: string;
    recordingId?: string | null;
    submissionPriority?: string;
  };
}

export const createSubmission = async (data: {
  title: string;
  description?: string;
  priority?: string;
  recordingId?: string;
}) => {
  const res = await api.post('/agent/submissions', data);
  return res.data as AgentSubmission;
};

export const listMySubmissions = async () => {
  const res = await api.get('/agent/submissions');
  return res.data as AgentSubmission[];
};
