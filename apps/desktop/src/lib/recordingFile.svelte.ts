import {
  cancelRecordingFile,
  closeRecordingFile,
  openRecordingFile,
  toUiError,
  type FileSummary,
  type OpenProgress,
} from "./backend";

/** The recording file in the File tab (FR AXHK2S): opening it with progress and Cancel, a
 * refusal in the backend's words, Close. One file at a time: a new one replaces the old,
 * and a dismissed dialog or a refusal keeps it. */
export class RecordingFile {
  /** The open file, null with none. */
  summary = $state.raw<FileSummary | null>(null);
  opening = $state(false);
  /** How far the open has got, null until it reports. */
  progress = $state.raw<OpenProgress | null>(null);
  /** Why the last open was refused, or the last close failed. */
  error = $state<string | null>(null);

  /** A file is being read. `opening` alone starts before the dialog closes, and nothing
   * shows over the dialog: the backend's first report comes once a file is chosen. */
  get working(): boolean {
    return this.opening && this.progress !== null;
  }

  /** Asks for a file and opens it; resolves to whether a new file is now open. A second
   * call while one runs is ignored. */
  async open(): Promise<boolean> {
    if (this.opening) return false;
    this.opening = true;
    this.progress = null;
    this.error = null;
    try {
      const opened = await openRecordingFile((p) => (this.progress = p));
      if (opened === null) return false;
      this.summary = opened;
      return true;
    } catch (err) {
      this.error = toUiError(err).message;
      return false;
    } finally {
      this.opening = false;
      this.progress = null;
    }
  }

  /** Ends the open under way, which then leaves no file. */
  async cancel(): Promise<void> {
    await cancelRecordingFile();
  }

  /** Closes the file here and in the backend; resolves to whether it closed. A failure
   * keeps the file open and says why. */
  async close(): Promise<boolean> {
    try {
      await closeRecordingFile();
    } catch (err) {
      this.error = toUiError(err).message;
      return false;
    }
    this.summary = null;
    this.error = null;
    return true;
  }
}
