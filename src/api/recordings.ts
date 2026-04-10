import api from './client';

export type AiAnalysisStatus = 'pending' | 'processing' | 'completed' | 'failed' | 'skipped';

export interface RecordingDevChange {
  title: string;
  description: string;
  changeType: 'bug' | 'feature' | 'enhancement' | 'question';
  affectedArea?: string;
  visualEvidence?: string;
}

export interface RecordingAnalysis {
  summary: string;
  scopeOfWork: string;
  suggestedTitle: string;
  suggestedDescription: string;
  suggestedPriority: 'low' | 'medium' | 'high' | 'critical';
  suggestedLabels: string[];
  devChanges: RecordingDevChange[];
  modelUsed: string;
  generatedAt: string;
}

export interface RecordingDetail {
  id: string;
  uploadedByUserId: string;
  originalFileName: string | null;
  filePath: string | null;
  fileSize: number | null;
  mimeType: string | null;
  durationSeconds: number | null;
  transcription: string | null;
  transcriptionStatus: 'pending' | 'completed' | 'skipped';
  uploadStatus: 'uploading' | 'completed' | 'failed';
  frameUrls: string[] | null;
  aiAnalysisStatus: AiAnalysisStatus | null;
  aiAnalysis: RecordingAnalysis | null;
  aiAnalysisError: string | null;
  aiAnalysisModel: string | null;
  aiAnalysisProvider: string | null;
  aiAnalysisStartedAt: string | null;
  aiAnalysisCompletedAt: string | null;
  aiAnalysisInputTokens: number | null;
  aiAnalysisOutputTokens: number | null;
  aiAnalysisImageTokens: number | null;
  aiAnalysisTotalTokens: number | null;
  aiAnalysisCostUsd: string | number | null;
  aiAnalysisLatencyMs: number | null;
  createdAt: string;
  updatedAt: string;
}

export const getRecording = async (id: string): Promise<RecordingDetail> => {
  const res = await api.get<RecordingDetail>(`/recordings/${id}`);
  return res.data;
};
