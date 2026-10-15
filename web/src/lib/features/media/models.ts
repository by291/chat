import type { components } from '#lib/api-client';

export type Attachment = components['schemas']['AttachmentView'];
export type UploadGrant = components['schemas']['UploadGrant'];
export type DownloadGrant = components['schemas']['DownloadGrant'];
export type UploadInput = components['schemas']['UploadInput'];

export type UploadScope = {
  purpose: 'message' | 'avatar';
  conversationId?: string;
  membershipId?: string;
};

export interface MediaApi {
  create(input: UploadInput, signal: AbortSignal): Promise<UploadGrant>;
  renew(id: string, signal: AbortSignal): Promise<UploadGrant>;
  complete(
    id: string,
    attempt: string,
    signal: AbortSignal,
  ): Promise<Attachment>;
  status(id: string, signal: AbortSignal): Promise<Attachment>;
  cancel(id: string): Promise<void>;
  download(
    id: string,
    variant: 'original' | 'thumbnail',
    signal?: AbortSignal,
  ): Promise<DownloadGrant>;
}
