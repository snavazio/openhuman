/**
 * Utilities for multimodal chat attachments.
 *
 * Images are embedded as `[IMAGE:<data-uri>]` markers. Other supported files
 * are embedded as `[FILE:<data-uri>]` markers. The Rust agent harness
 * (`agent/multimodal.rs`) parses, validates, and expands both shapes before
 * the provider call.
 */
import debugFactory from 'debug';

import { transcribeWithFactory } from '../features/human/voice/sttClient';

const debug = debugFactory('chat:attachments');

const ALLOWED_IMAGE_MIME_TYPES = [
  'image/png',
  'image/jpeg',
  'image/webp',
  'image/gif',
  'image/bmp',
] as const;

export type AllowedImageMimeType = (typeof ALLOWED_IMAGE_MIME_TYPES)[number];

const ALLOWED_FILE_MIME_TYPES = [
  'application/pdf',
  'text/plain',
  'text/csv',
  'text/markdown',
  'application/zip',
  'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  'application/octet-stream',
] as const;

export type AllowedFileMimeType = (typeof ALLOWED_FILE_MIME_TYPES)[number];

// Video formats accepted by the composer. The original video is never sent to
// the provider — instead we sample a few still frames client-side and forward
// them through the existing `[IMAGE:]` vision path (see `extractVideoFrames` +
// `buildMessageWithAttachments`). So a vision-capable tier is required, the same
// gate as images; audio and motion between frames are not conveyed.
const ALLOWED_VIDEO_MIME_TYPES = ['video/mp4', 'video/quicktime', 'video/webm'] as const;

export type AllowedVideoMimeType = (typeof ALLOWED_VIDEO_MIME_TYPES)[number];
export type AllowedAttachmentMimeType =
  | AllowedImageMimeType
  | AllowedFileMimeType
  | AllowedVideoMimeType;
export type AttachmentKind = 'image' | 'file' | 'video';

export const ALLOWED_ATTACHMENT_MIME_TYPES = [
  ...ALLOWED_IMAGE_MIME_TYPES,
  ...ALLOWED_FILE_MIME_TYPES,
  ...ALLOWED_VIDEO_MIME_TYPES,
] as const;

// Document formats the backend actually text-extracts (PDF via pdf_extract;
// TXT/Markdown via UTF-8). DOCX/PPTX/XLSX/ZIP are intentionally excluded — the
// agent would only see a reference stub, not their content. `text/csv` is also
// deliberately left out: the backend *can* extract it, but the chat composer is
// scoped to PDF/TXT/Markdown by product decision (revisit here if CSV is wanted).
// Used by the ingest validator below, not by a native `accept` filter:
// Chromium/CEF on macOS greys valid files at the open panel regardless of the
// filter shape, so selection is gated in `validateAndReadFile` after the user
// picks, not at the dialog.
const EXTRACTABLE_FILE_MIME_TYPES = ['application/pdf', 'text/plain', 'text/markdown'] as const;

// Shared image-marker budget per message. Images cost 1 marker each; a video
// costs VIDEO_FRAME_COUNT markers (its sampled frames). Mirrors the core default
// `multimodal.max_images` (crates/openhuman-core/src/config/schema/tools/multimodal.rs) — the
// core counts every `[IMAGE:]` marker (frames included) and errors on overflow,
// so the composer must budget images + video frames against this single cap.
// LOCAL-ONLY FORK: 12, matching `[multimodal] max_images = 12` in the evo-x2
// profile, so one video's VIDEO_FRAME_COUNT frames fit beside a few images.
export const ATTACHMENT_MAX_IMAGES = 12;
export const ATTACHMENT_MAX_FILES = 4;
export const ATTACHMENT_MAX_IMAGE_SIZE_BYTES = 8 * 1024 * 1024; // 8 MB
export const ATTACHMENT_MAX_FILE_SIZE_BYTES = 16 * 1024 * 1024; // 16 MB
export const ATTACHMENT_MAX_VIDEO_SIZE_BYTES = 50 * 1024 * 1024; // 50 MB
// Still frames sampled from a video and forwarded as `[IMAGE:]` markers.
// LOCAL-ONLY FORK: 8 (was 2) so the vision model sees the clip's progression,
// not just two moments; 1 video + 4 images fits the 12-marker budget.
export const VIDEO_FRAME_COUNT = 8;
// Shorter side of a sampled frame. Gemma 3 resizes every image to 896x896, so
// capping the *longest* side squeezed a portrait phone recording to ~470 px wide
// and on-screen prices became unreadable. Keep the short side at the model's
// input size and only shrink larger sources.
export const VIDEO_FRAME_SHORT_EDGE = 896;

export interface Attachment {
  id: string;
  kind: AttachmentKind;
  file: File;
  dataUri: string;
  previewUri?: string;
  mimeType: AllowedAttachmentMimeType;
  originalSizeBytes: number;
  payloadSizeBytes: number;
  compressed: boolean;
  // Only set for `kind: 'video'`: the still frames sampled from the clip,
  // expanded into `[IMAGE:]` markers at send time. The chip itself shows a
  // single poster (the first frame) via `previewUri`.
  frames?: string[];
  // LOCAL-ONLY FORK: only set for `kind: 'video'`. The clip's spoken audio,
  // transcribed in the background from the moment it is attached (see
  // `startVideoTranscript`). Never rejects; resolved at send time by
  // `resolveVideoTranscripts`.
  transcript?: Promise<VideoTranscript>;
}

/** Outcome of transcribing a video's audio: the text, or why there is none. */
export type VideoTranscript = { text: string } | { note: string };

type AttachmentError =
  | { code: 'unsupported_type'; mimeType: string }
  | { code: 'too_large'; sizeBytes: number; maxBytes: number }
  | { code: 'too_many'; kind: AttachmentKind; max: number }
  | { code: 'image_not_supported' }
  | { code: 'video_not_supported' }
  | { code: 'read_failed'; reason: string };

export function isAllowedMimeType(mime: string): mime is AllowedImageMimeType {
  return (ALLOWED_IMAGE_MIME_TYPES as readonly string[]).includes(mime);
}

export function isVideoMimeType(mime: string): mime is AllowedVideoMimeType {
  return (ALLOWED_VIDEO_MIME_TYPES as readonly string[]).includes(mime);
}

/**
 * The exact MIME set the ingest validator accepts — images plus the
 * text-extractable documents. A strict subset of {@link AllowedAttachmentMimeType}
 * (which also lists reference-only types like CSV/DOCX/ZIP that we reject).
 */
type SupportedAttachmentMimeType =
  | AllowedImageMimeType
  | AllowedVideoMimeType
  | (typeof EXTRACTABLE_FILE_MIME_TYPES)[number];

/**
 * Only accepts formats the backend actually reads — images, plus the
 * text-extractable documents (PDF via
 * pdf_extract; TXT/Markdown via UTF-8). DOCX/PPTX/XLSX/ZIP are excluded so they
 * can't be attached as content-less reference stubs. Applied on every ingest
 * path (picker, drag-drop, paste).
 */
function isSupportedAttachmentMimeType(mime: string): mime is SupportedAttachmentMimeType {
  return (
    (ALLOWED_IMAGE_MIME_TYPES as readonly string[]).includes(mime) ||
    (ALLOWED_VIDEO_MIME_TYPES as readonly string[]).includes(mime) ||
    (EXTRACTABLE_FILE_MIME_TYPES as readonly string[]).includes(mime)
  );
}

export function attachmentKindForMime(mime: AllowedAttachmentMimeType): AttachmentKind {
  if (isAllowedMimeType(mime)) return 'image';
  if (isVideoMimeType(mime)) return 'video';
  return 'file';
}

export function fileToDataUri(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    const name = file instanceof File ? file.name : 'blob';
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(new Error(`Failed to read file: ${name}`));
    reader.readAsDataURL(file);
  });
}

async function blobToDataUri(blob: Blob, mimeType: string): Promise<string> {
  const namedBlob = new Blob([blob], { type: mimeType });
  return fileToDataUri(namedBlob);
}

async function gzipBlob(file: File): Promise<Blob | null> {
  if (!('CompressionStream' in globalThis)) return null;

  try {
    const compressionStream = new CompressionStream('gzip');
    const compressed = file.stream().pipeThrough(compressionStream);
    return await new Response(compressed).blob();
  } catch (error) {
    debug('[chat:attachments] gzip_failed name=%s error=%o', file.name, error);
    return null;
  }
}

function encodeDataUriParam(value: string): string {
  return encodeURIComponent(value).replace(/'/g, '%27');
}

async function buildAttachmentDataUri(
  file: File,
  mimeType: AllowedAttachmentMimeType
): Promise<{ dataUri: string; payloadSizeBytes: number; compressed: boolean }> {
  debug(
    '[chat:attachments] compression:start name=%s mime=%s size=%d',
    file.name,
    mimeType,
    file.size
  );

  const compressed = await gzipBlob(file);
  if (compressed && compressed.size < file.size) {
    const dataUri = await blobToDataUri(
      compressed,
      `application/gzip;original_mime=${encodeDataUriParam(mimeType)};name=${encodeDataUriParam(file.name)}`
    );
    debug(
      '[chat:attachments] compression:ok name=%s original=%d compressed=%d',
      file.name,
      file.size,
      compressed.size
    );
    return { dataUri, payloadSizeBytes: compressed.size, compressed: true };
  }

  const dataUri = await fileToDataUri(file);
  debug(
    '[chat:attachments] compression:skipped name=%s original=%d compressed=%s',
    file.name,
    file.size,
    compressed?.size ?? 'unavailable'
  );
  return { dataUri, payloadSizeBytes: file.size, compressed: false };
}

/** Evenly spread `count` sample points across the 0.1–0.9 span of the clip. */
function sampleFractions(count: number): number[] {
  if (count <= 1) return [0.1];
  const fractions: number[] = [];
  for (let i = 0; i < count; i++) {
    fractions.push(0.1 + (0.8 * i) / (count - 1));
  }
  return fractions;
}

function seekVideo(video: HTMLVideoElement, time: number): Promise<void> {
  return new Promise((resolve, reject) => {
    // Assigning currentTime to (approximately) its current value is a no-op and
    // never fires `seeked` (HTML spec) — which would hang for zero-length clips
    // or a first frame already at t=0. Short-circuit those.
    if (Math.abs(video.currentTime - time) < 0.01) {
      resolve();
      return;
    }
    const cleanup = () => {
      video.removeEventListener('seeked', onSeeked);
      video.removeEventListener('error', onError);
    };
    const onSeeked = () => {
      cleanup();
      resolve();
    };
    const onError = () => {
      cleanup();
      reject(new Error('video seek failed'));
    };
    video.addEventListener('seeked', onSeeked);
    video.addEventListener('error', onError);
    video.currentTime = time;
  });
}

/**
 * Sample `count` still frames from a video file as JPEG data URIs by decoding it
 * in a detached `<video>` element and painting each seek point onto a `<canvas>`.
 * The full clip is never uploaded — only these frames ride the `[IMAGE:]` vision
 * path. Throws if the browser can't decode the file (the caller maps that to a
 * `read_failed` error). Requires a real codec-capable runtime (CEF/Chromium);
 * jsdom can't decode video, so unit tests stub {@link videoFrameExtractor}.
 */
async function extractVideoFramesImpl(
  file: File,
  count: number = VIDEO_FRAME_COUNT
): Promise<string[]> {
  const url = URL.createObjectURL(file);
  const video = document.createElement('video');
  video.muted = true;
  video.preload = 'auto';
  video.src = url;
  try {
    await new Promise<void>((resolve, reject) => {
      video.onloadedmetadata = () => resolve();
      video.onerror = () => reject(new Error('video metadata load failed'));
    });
    const duration = Number.isFinite(video.duration) && video.duration > 0 ? video.duration : 0;
    const canvas = document.createElement('canvas');
    const srcW = video.videoWidth || 320;
    const srcH = video.videoHeight || 240;
    const scale = Math.min(1, VIDEO_FRAME_SHORT_EDGE / Math.min(srcW, srcH));
    canvas.width = Math.round(srcW * scale);
    canvas.height = Math.round(srcH * scale);
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('canvas 2d context unavailable');

    const frames: string[] = [];
    for (const fraction of sampleFractions(count)) {
      const target = duration ? Math.min(duration * fraction, Math.max(duration - 0.05, 0)) : 0;
      await seekVideo(video, target);
      ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
      frames.push(canvas.toDataURL('image/jpeg', 0.7));
    }
    debug('[chat:attachments] video_frames name=%s count=%d', file.name, frames.length);
    return frames;
  } finally {
    URL.revokeObjectURL(url);
    video.removeAttribute('src');
  }
}

/**
 * Indirection seam so unit tests can stub frame extraction (jsdom has no video
 * decoder). Production calls `extract` directly.
 */
export const videoFrameExtractor = { extract: extractVideoFramesImpl };

// LOCAL-ONLY FORK: videos above this size are not transcribed. The clip is sent
// base64-encoded (+33%) in one JSON-RPC body, and the core caps bodies at 64 MB.
export const ATTACHMENT_MAX_TRANSCRIBE_BYTES = 40 * 1024 * 1024; // 40 MB

// Speech models want 16 kHz mono; sending only that keeps the upload small.
const SPEECH_SAMPLE_RATE = 16_000;
// Assume the link to the core manages at least this many bytes per second.
// A remote core over a slow relay cannot take a whole video in the default 30s.
const MIN_UPLOAD_BYTES_PER_SEC = 20_000;

/** RPC budget for an STT upload of `bytes`: 30s plus time to send the base64 body. */
export function transcribeTimeoutMs(bytes: number): number {
  const base64Bytes = Math.ceil(bytes / 3) * 4;
  return 30_000 + Math.ceil(base64Bytes / MIN_UPLOAD_BYTES_PER_SEC) * 1000;
}

/** Encode mono float samples as a 16-bit PCM WAV blob. */
export function encodeWav16(samples: Float32Array, sampleRate: number): Blob {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  const writeAscii = (offset: number, text: string) => {
    for (let i = 0; i < text.length; i++) view.setUint8(offset + i, text.charCodeAt(i));
  };
  writeAscii(0, 'RIFF');
  view.setUint32(4, 36 + samples.length * 2, true);
  writeAscii(8, 'WAVE');
  writeAscii(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeAscii(36, 'data');
  view.setUint32(40, samples.length * 2, true);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return new Blob([buffer], { type: 'audio/wav' });
}

/**
 * Decode a video's audio track in the browser and return it as 16 kHz mono WAV,
 * usually a small fraction of the video's size. Returns null when the browser
 * has no Web Audio decoder (tests, old webviews); throws when the file has no
 * audio track the browser can decode.
 */
async function extractSpeechAudioImpl(file: File): Promise<Blob | null> {
  if (typeof OfflineAudioContext === 'undefined') return null;
  // decodeAudioData resamples to the context's rate.
  const ctx = new OfflineAudioContext(1, 1, SPEECH_SAMPLE_RATE);
  const decoded = await ctx.decodeAudioData(await file.arrayBuffer());
  const mono = new Float32Array(decoded.length);
  for (let ch = 0; ch < decoded.numberOfChannels; ch++) {
    const data = decoded.getChannelData(ch);
    for (let i = 0; i < data.length; i++) mono[i] += data[i] / decoded.numberOfChannels;
  }
  return encodeWav16(mono, decoded.sampleRate);
}

/** Indirection seam so unit tests can stub audio extraction (jsdom has no decoder). */
export const speechAudioExtractor = { extract: extractSpeechAudioImpl };

/**
 * Indirection seam so unit tests can stub transcription. Production extracts
 * the speech audio in the browser and sends only that to
 * `openhuman.voice_stt_dispatch`; without a decoder it sends the clip itself
 * and the STT provider (whisper with ffmpeg conversion) extracts the audio.
 */
export const videoTranscriber = {
  transcribe: async (file: File): Promise<string> => {
    let audio: Blob | null;
    try {
      audio = await speechAudioExtractor.extract(file);
    } catch (err) {
      // No audio track, or a codec this browser cannot decode (Chromium builds
      // without AAC). Let the STT provider's ffmpeg try the clip itself.
      debug(
        '[attachments] video %s: browser could not decode audio, sending clip: %O',
        file.name,
        err
      );
      audio = null;
    }
    const upload = audio ?? file;
    const fileName = audio ? `${file.name.replace(/\.[^.]+$/, '')}.wav` : file.name;
    debug(
      '[attachments] video %s: uploading %s (%d bytes, video was %d)',
      file.name,
      audio ? 'extracted speech audio' : 'whole clip',
      upload.size,
      file.size
    );
    return transcribeWithFactory(upload, {
      mimeType: upload.type,
      fileName,
      timeoutMs: transcribeTimeoutMs(upload.size),
    });
  },
};

/**
 * Start transcribing a video's audio. The returned promise never rejects: a
 * missing transcript must not block the message, so failures become a note
 * the agent is told about instead.
 */
export function startVideoTranscript(file: File): Promise<VideoTranscript> {
  if (file.size > ATTACHMENT_MAX_TRANSCRIBE_BYTES) {
    debug('[attachments] video %s too large to transcribe (%d bytes)', file.name, file.size);
    return Promise.resolve({ note: 'the video is too large to transcribe (over 40 MB)' });
  }
  const started = Date.now();
  debug('[attachments] transcribing video %s (%d bytes)', file.name, file.size);
  return videoTranscriber
    .transcribe(file)
    .then((text): VideoTranscript => {
      const trimmed = text.trim();
      debug(
        '[attachments] video %s transcript chars=%d ms=%d',
        file.name,
        trimmed.length,
        Date.now() - started
      );
      return trimmed ? { text: trimmed } : { note: 'no speech was detected in the video' };
    })
    .catch((err: unknown): VideoTranscript => {
      const reason = err instanceof Error ? err.message : String(err);
      debug('[attachments] video %s transcription failed: %s', file.name, reason);
      return { note: `the video could not be transcribed (${reason})` };
    });
}

/**
 * Wait for every video attachment's transcript and render them as plain text
 * for the agent, one block per video. Returns '' when there are no videos.
 */
export async function resolveVideoTranscripts(attachments: Attachment[]): Promise<string> {
  const videos = attachments.filter(a => a.kind === 'video' && a.transcript);
  if (videos.length === 0) return '';
  const blocks = await Promise.all(
    videos.map(async a => {
      const result = await (a.transcript as Promise<VideoTranscript>);
      return 'text' in result
        ? `[Transcript of ${a.file.name}]\n${result.text}`
        : `[No transcript for ${a.file.name}: ${result.note}]`;
    })
  );
  return blocks.join('\n\n');
}

export function extractVideoFrames(file: File, count?: number): Promise<string[]> {
  return videoFrameExtractor.extract(file, count);
}

/** Image-marker cost of an attachment kind (video = its sampled frames). */
export function imageMarkerCost(kind: AttachmentKind): number {
  if (kind === 'image') return 1;
  if (kind === 'video') return VIDEO_FRAME_COUNT;
  return 0;
}

export async function validateAndReadFile(
  file: File,
  // Image-marker slots already consumed this message: 1 per image + VIDEO_FRAME_COUNT
  // per video. Images and videos share one budget (ATTACHMENT_MAX_IMAGES) because
  // the core counts every `[IMAGE:]` marker — frames included — and rejects overflow.
  existingImageMarkers: number,
  existingFileCount = 0,
  // When `false` (the active chat model isn't vision-capable), image AND video
  // files are rejected (video is conveyed as sampled frames through the vision
  // path); documents (PDF/Word/etc.) still flow. Defaults `true` so non-chat
  // callers are unaffected.
  allowImages = true
): Promise<{ attachment: Attachment } | { error: AttachmentError }> {
  if (!isSupportedAttachmentMimeType(file.type)) {
    return { error: { code: 'unsupported_type', mimeType: file.type || 'unknown' } };
  }

  const kind = attachmentKindForMime(file.type);
  if (!allowImages && kind === 'image') {
    return { error: { code: 'image_not_supported' } };
  }
  if (!allowImages && kind === 'video') {
    return { error: { code: 'video_not_supported' } };
  }

  if (kind === 'file') {
    if (existingFileCount >= ATTACHMENT_MAX_FILES) {
      return { error: { code: 'too_many', kind: 'file', max: ATTACHMENT_MAX_FILES } };
    }
  } else {
    // image or video: budget against the shared image-marker cap.
    if (existingImageMarkers + imageMarkerCost(kind) > ATTACHMENT_MAX_IMAGES) {
      return { error: { code: 'too_many', kind: 'image', max: ATTACHMENT_MAX_IMAGES } };
    }
  }

  const maxBytes =
    kind === 'image'
      ? ATTACHMENT_MAX_IMAGE_SIZE_BYTES
      : kind === 'video'
        ? ATTACHMENT_MAX_VIDEO_SIZE_BYTES
        : ATTACHMENT_MAX_FILE_SIZE_BYTES;
  if (file.size > maxBytes) {
    return { error: { code: 'too_large', sizeBytes: file.size, maxBytes } };
  }

  try {
    if (kind === 'video') {
      const frames = await videoFrameExtractor.extract(file);
      if (frames.length === 0) {
        return { error: { code: 'read_failed', reason: 'no frames extracted' } };
      }
      return {
        attachment: {
          id: globalThis.crypto.randomUUID(),
          kind: 'video',
          file,
          // The clip is represented to the agent by its frames, not a data URI;
          // the poster (first frame) drives the chip thumbnail.
          dataUri: frames[0],
          previewUri: frames[0],
          mimeType: file.type,
          originalSizeBytes: file.size,
          payloadSizeBytes: file.size,
          compressed: false,
          frames,
          transcript: startVideoTranscript(file),
        },
      };
    }

    const { dataUri, payloadSizeBytes, compressed } = await buildAttachmentDataUri(file, file.type);
    const previewUri = kind === 'image' ? await fileToDataUri(file) : undefined;
    return {
      attachment: {
        id: globalThis.crypto.randomUUID(),
        kind,
        file,
        dataUri,
        previewUri,
        mimeType: file.type,
        originalSizeBytes: file.size,
        payloadSizeBytes,
        compressed,
      },
    };
  } catch (err) {
    return {
      error: { code: 'read_failed', reason: err instanceof Error ? err.message : String(err) },
    };
  }
}

/**
 * Compose the final message string by appending `[IMAGE:<data-uri>]` markers
 * for image attachments and `[FILE:<data-uri>]` markers for other supported
 * files after the user's text. The Rust agent harness parses and strips these
 * markers before forwarding clean text and attachment payloads to the provider.
 */
export function buildMessageWithAttachments(text: string, attachments: Attachment[]): string {
  if (attachments.length === 0) return text;
  const markers = attachments
    .map(a => {
      if (a.kind === 'image') return `[IMAGE:${a.dataUri}]`;
      if (a.kind === 'file') return `[FILE:${a.dataUri}]`;
      // Video: forward each sampled still as its own image marker so the agent
      // "sees" the clip through the existing vision path.
      return (a.frames ?? []).map(frame => `[IMAGE:${frame}]`).join(' ');
    })
    .filter(marker => marker.length > 0)
    .join(' ');
  return text.trim() ? `${text.trim()} ${markers}` : markers;
}

/**
 * Parse `[IMAGE:<data-uri>]` and `[FILE:<data-uri>]` markers out of a stored message string.
 * Returns the clean text (markers removed) and the list of image data URIs found.
 * File markers are stripped from text but not returned (file data lives in extraMetadata).
 */
export function parseMessageImages(content: string): { text: string; dataUris: string[] } {
  const dataUris: string[] = [];
  const text = content
    .replace(/\[IMAGE:([^\]]+)\]/g, (_match, uri: string) => {
      dataUris.push(uri);
      return '';
    })
    .replace(/\[FILE:([^\]]+)\]/g, '') // Strip file markers
    // Collapse only runs of plain spaces (not \s) left behind by marker
    // removal — using \s here would also eat intentional newlines/paragraph
    // breaks in the user's own text.
    .replace(/ {2,}/g, ' ')
    .trim();
  return { text, dataUris };
}

export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
