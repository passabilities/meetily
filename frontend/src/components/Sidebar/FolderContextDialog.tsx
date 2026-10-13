'use client';

import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from '@/components/ui/dialog';
import { SUMMARY_CONTEXT_MAX_CHARS, summaryContextCounter } from '@/lib/summary-context';
import { summaryContextService } from '@/services/summaryContextService';

type Folder = { id: string; name: string };

interface FolderContextDialogProps {
  /** The folder being edited, or null while the dialog is closed. */
  folder: Folder | null;
  onClose: () => void;
}

/** Edits the summary context of one folder. */
export function FolderContextDialog({ folder, onClose }: FolderContextDialogProps) {
  // Keep showing the last folder while the dialog fades out after `folder` becomes null.
  const shownFolder = useRef<Folder | null>(null);
  if (folder) shownFolder.current = folder;
  const content = folder ?? shownFolder.current;

  return (
    <Dialog open={folder !== null} onOpenChange={open => !open && onClose()}>
      {/* A click outside must not discard what was typed; Esc, Cancel and the close button still close. */}
      <DialogContent className="sm:max-w-lg" onInteractOutside={event => event.preventDefault()}>
        {content && <FolderContextForm key={content.id} folder={content} onClose={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

function FolderContextForm({ folder, onClose }: { folder: Folder; onClose: () => void }) {
  const [text, setText] = useState('');
  const [isLoaded, setIsLoaded] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const folderId = folder.id;

  useEffect(() => {
    let cancelled = false;
    summaryContextService.getFolder(folderId)
      .then(saved => {
        if (cancelled) return;
        setText(saved);
        setIsLoaded(true);
      })
      .catch(error => {
        if (cancelled) return;
        // Saving over text that could not be read would lose it, so close instead.
        console.error('Failed to load summary context:', error);
        toast.error('Failed to load summary context', { description: String(error) });
        onClose();
      });
    return () => {
      cancelled = true;
    };
    // Load once per opening, even if the parent passes a new onClose on each render.
  }, [folderId]);

  // The text area is disabled until the text has loaded, so it can only take focus afterwards.
  useEffect(() => {
    if (isLoaded) textareaRef.current?.focus();
  }, [isLoaded]);

  const save = async () => {
    setIsSaving(true);
    try {
      await summaryContextService.saveFolder(folderId, text);
      onClose();
    } catch (error) {
      console.error('Failed to save summary context:', error);
      toast.error('Failed to save summary context', { description: String(error) });
    } finally {
      setIsSaving(false);
    }
  };

  const counter = summaryContextCounter(text.length);

  return (
    <>
      <DialogTitle>Summary context for &quot;{folder.name}&quot;</DialogTitle>
      <DialogDescription className="text-sm text-gray-600">
        Sent with the summary of every meeting in this folder.
      </DialogDescription>
      <textarea
        ref={textareaRef}
        aria-label="Summary context for this folder"
        className="w-full px-3 py-2 border border-gray-200 rounded-md text-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 bg-white shadow-sm min-h-[140px] resize-y disabled:bg-gray-50"
        value={text}
        onChange={e => setText(e.target.value)}
        disabled={!isLoaded}
        maxLength={SUMMARY_CONTEXT_MAX_CHARS}
      />
      {counter && <div className="text-right text-xs text-gray-500">{counter}</div>}
      <DialogFooter>
        <Button variant="outline" onClick={onClose} disabled={isSaving}>Cancel</Button>
        <Button onClick={() => void save()} disabled={!isLoaded || isSaving}>Save</Button>
      </DialogFooter>
    </>
  );
}
