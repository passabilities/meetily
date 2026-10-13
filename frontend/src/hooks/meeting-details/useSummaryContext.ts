import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { createDebouncedSaver } from '@/lib/debouncedSaver';
import { SUMMARY_CONTEXT_SAVE_DELAY_MS, type InheritedContext } from '@/lib/summary-context';
import { summaryContextService } from '@/services/summaryContextService';
import { organizationService } from '@/services/organizationService';

interface SavedValue {
  context: string;
  useInherited: boolean;
}

const EMPTY: SavedValue = { context: '', useInherited: true };

/** Loads and autosaves the summary context of the open meeting. */
export function useSummaryContext(
  meetingId: string,
  saveDelayMs: number = SUMMARY_CONTEXT_SAVE_DELAY_MS,
) {
  const [value, setValue] = useState<SavedValue>(EMPTY);
  const [inherited, setInherited] = useState<InheritedContext[]>([]);
  const [isLoaded, setIsLoaded] = useState(false);
  const valueRef = useRef<SavedValue>(EMPTY);
  const loadedRef = useRef(false);
  const meetingIdRef = useRef(meetingId);
  meetingIdRef.current = meetingId;

  const [saver] = useState(() => createDebouncedSaver<SavedValue>(
    (id, next) => summaryContextService.saveMeeting(id, next.context, next.useInherited),
    error => {
      console.error('Failed to save summary context:', error);
      toast.error('Failed to save summary context', { description: String(error) });
    },
    saveDelayMs,
  ));

  const apply = useCallback((next: SavedValue) => {
    valueRef.current = next;
    setValue(next);
  }, []);

  useEffect(() => {
    let cancelled = false;
    apply(EMPTY);
    setInherited([]);
    setIsLoaded(false);
    loadedRef.current = false;

    summaryContextService.getMeeting(meetingId)
      .then(result => {
        if (cancelled) return;
        apply({ context: result.context, useInherited: result.useInherited });
        setInherited(result.inherited);
      })
      .catch(error => {
        if (cancelled) return;
        console.error('Failed to load summary context:', error);
        toast.error('Failed to load summary context', { description: String(error) });
      })
      .finally(() => {
        if (cancelled) return;
        loadedRef.current = true;
        setIsLoaded(true);
      });

    return () => {
      cancelled = true;
      // The pending save carries this meeting's id, so it cannot land on the next meeting.
      void saver.flush();
    };
  }, [meetingId, saver, apply]);

  const setText = useCallback((context: string) => {
    if (!loadedRef.current) return;
    const next = { ...valueRef.current, context };
    apply(next);
    saver.schedule(meetingId, next);
  }, [apply, meetingId, saver]);

  const setUseInherited = useCallback((useInherited: boolean) => {
    if (!loadedRef.current) return;
    const next = { ...valueRef.current, useInherited };
    apply(next);
    saver.schedule(meetingId, next);
    void saver.flush();
  }, [apply, meetingId, saver]);

  const flush = useCallback(() => saver.flush(), [saver]);

  /** Refreshes the inherited levels only; the text and switch on screen are left alone. */
  const reload = useCallback(async () => {
    try {
      const result = await summaryContextService.getMeeting(meetingId);
      if (meetingIdRef.current === meetingId) setInherited(result.inherited);
    } catch (error) {
      console.error('Failed to refresh inherited summary context:', error);
    }
  }, [meetingId]);

  // Folder context edits and moving the meeting to another folder change what is inherited.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    organizationService.onMeetingsChanged(() => { void reload(); })
      .then(stop => {
        if (cancelled) stop(); else unlisten = stop;
      })
      .catch(error => console.error('Failed to listen for meetings-changed:', error));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [reload]);

  return {
    text: value.context,
    setText,
    useInherited: value.useInherited,
    setUseInherited,
    inherited,
    isLoaded,
    flush,
    reload,
  };
}
