'use client';

import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogFooter, DialogTitle } from '@/components/ui/dialog';
import { SUMMARY_CONTEXT_MAX_CHARS, summaryContextCounter } from '@/lib/summary-context';
import { summaryContextService } from '@/services/summaryContextService';

interface FolderContextDialogProps {
  /** The folder being edited, or null while the dialog is closed. */
  folder: { id: string; name: string } | null;
  onClose: () => void;
}

/** Edits the summary context of one folder. */
export function FolderContextDialog({ folder, onClose }: FolderContextDialogProps) {
  const [text, setText] = useState('');
  const [isLoaded, setIsLoaded] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const folderId = folder?.id ?? null;

  useEffect(() => {
    if (!folderId) return;
    let cancelled = false;
    setText('');
    setIsLoaded(false);
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
  }, [folderId, onClose]);

  const save = async () => {
    if (!folderId) return;
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
    <Dialog open={folder !== null} onOpenChange={open => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogTitle>Summary context for &quot;{folder?.name}&quot;</DialogTitle>
        <p className="text-sm text-gray-600">Sent with the summary of every meeting in this folder.</p>
        <textarea
          autoFocus
          aria-label="Summary context for this folder"
          className="w-full px-3 py-2 border border-gray-200 rounded-md text-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 bg-white shadow-sm min-h-[140px] resize-y disabled:bg-gray-50"
          value={text}
          onChange={e => setText(e.target.value)}
          disabled={!isLoaded}
          maxLength={SUMMARY_CONTEXT_MAX_CHARS}
        />
        {counter && <div className="text-right text-xs text-gray-500">{counter}</div>}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>Cancel</Button>
          <Button onClick={() => void save()} disabled={!isLoaded || isSaving}>Save</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
