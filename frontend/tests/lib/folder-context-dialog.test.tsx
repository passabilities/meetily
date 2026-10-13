import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { ReactNode } from 'react';

// Bun shares module mocks between test files; restore the real modules after this suite.
const originalCore = { ...await import('@tauri-apps/api/core') };
const originalToast = { ...await import('sonner') };
const originalDialog = { ...await import('../../src/components/ui/dialog') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('sonner', () => originalToast);
  mock.module('../../src/components/ui/dialog', () => originalDialog);
});

const toastError = mock((_message: string, _options?: unknown) => {});
mock.module('sonner', () => ({ toast: { error: toastError, info() {}, success() {}, warning() {} } }));

type SaveArgs = { folderId: string; context: string };
let load: (folderId: string) => Promise<{ context: string }>;
let save: (args: SaveArgs) => Promise<void>;
const saved: SaveArgs[] = [];
const loadedFolders: string[] = [];
const invoke = mock(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'api_get_folder_summary_context') {
    loadedFolders.push(args!.folderId as string);
    return load(args!.folderId as string);
  }
  if (command === 'api_save_folder_summary_context') {
    saved.push(args as SaveArgs);
    return save(args as SaveArgs);
  }
  throw new Error(`Unexpected command: ${command}`);
});
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));

let contentProps: Record<string, unknown> | undefined;
mock.module('../../src/components/ui/dialog', () => ({
  ...originalDialog,
  Dialog: ({ open, children }: { open: boolean; children?: ReactNode }) => (open ? <div>{children}</div> : null),
  DialogContent: ({ children, ...props }: { children?: ReactNode }) => {
    contentProps = props;
    return <section>{children}</section>;
  },
  DialogTitle: ({ children }: { children?: ReactNode }) => <h2>{children}</h2>,
  DialogDescription: ({ children }: { children?: ReactNode }) => <p>{children}</p>,
  DialogFooter: ({ children }: { children?: ReactNode }) => <footer>{children}</footer>,
}));

const { FolderContextDialog } = await import('../../src/components/Sidebar/FolderContextDialog');

const folder = { id: 'folder-1', name: 'Vendor sync' };
const onClose = mock(() => {});
const focus = mock(() => {});
let renderer: ReactTestRenderer;

const settle = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
async function show(value: typeof folder | null = folder) {
  await act(async () => {
    renderer = create(<FolderContextDialog folder={value} onClose={onClose} />, {
      createNodeMock: element => (element.type === 'textarea' ? { focus } : null),
    });
  });
  await settle();
}
const textarea = () => renderer.root.findByType('textarea');
const button = (label: string) =>
  renderer.root.findAll(node => node.type === 'button' && node.children.includes(label))[0];
const press = (label: string) => act(async () => { button(label).props.onClick(); });
const type = (value: string) => act(async () => { textarea().props.onChange({ target: { value } }); });

beforeEach(() => {
  saved.length = 0;
  loadedFolders.length = 0;
  contentProps = undefined;
  toastError.mockClear();
  onClose.mockClear();
  focus.mockClear();
  load = async () => ({ context: '' });
  save = async () => {};
});

describe('FolderContextDialog', () => {
  test('shows the saved text and lets the user type straight away', async () => {
    let finishLoad!: (value: { context: string }) => void;
    load = () => new Promise(resolve => { finishLoad = resolve; });
    await show();
    expect(textarea().props.disabled).toBe(true);
    expect(button('Save').props.disabled).toBe(true);
    expect(focus).not.toHaveBeenCalled();

    await act(async () => { finishLoad({ context: 'Weekly sync.' }); });
    await settle();
    expect(textarea().props.value).toBe('Weekly sync.');
    expect(textarea().props.disabled).toBe(false);
    expect(focus).toHaveBeenCalled();
    expect(button('Save').props.disabled).toBe(false);
  });

  test('names the folder in the title and describes itself', async () => {
    await show();
    const text = (type: string) =>
      renderer.root.findByType(type as 'h2').children.join('');
    expect(text('h2')).toBe('Summary context for "Vendor sync"');
    expect(text('p')).toBe('Sent with the summary of every meeting in this folder.');
  });

  test('saves the typed text and closes', async () => {
    await show();
    await type('New text');
    await press('Save');
    await settle();
    expect(saved).toEqual([{ folderId: 'folder-1', context: 'New text' }]);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  test('a failed save keeps the dialog open', async () => {
    save = async () => { throw new Error('disk full'); };
    await show();
    await type('New text');
    await press('Save');
    await settle();
    expect(toastError.mock.calls[0][0]).toBe('Failed to save summary context');
    expect(onClose).not.toHaveBeenCalled();
    expect(textarea().props.value).toBe('New text');
  });

  test('a failed load closes the dialog and saves nothing', async () => {
    load = async () => { throw new Error('unreadable'); };
    await show();
    expect(toastError.mock.calls[0][0]).toBe('Failed to load summary context');
    expect(onClose).toHaveBeenCalled();
    expect(saved).toEqual([]);
  });

  test('a click outside does not close it', async () => {
    await show();
    const onInteractOutside = contentProps?.onInteractOutside as ((event: unknown) => void) | undefined;
    expect(typeof onInteractOutside).toBe('function');
    const preventDefault = mock(() => {});
    onInteractOutside!({ preventDefault });
    expect(preventDefault).toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  test('renders nothing and loads nothing without a folder', async () => {
    await show(null);
    expect(renderer.toJSON()).toBeNull();
    expect(loadedFolders).toEqual([]);
  });

  test('Cancel is disabled while a save is in flight', async () => {
    let finishSave!: () => void;
    save = () => new Promise<void>(resolve => { finishSave = resolve; });
    await show();
    await press('Save');
    expect(button('Cancel').props.disabled).toBe(true);
    await act(async () => { finishSave(); });
    await settle();
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
