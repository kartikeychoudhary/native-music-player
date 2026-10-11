/**
 * Cancels dragover/drop at the document so a file dropped outside a drop zone never makes the
 * window navigate to it. Bubble-phase listeners, so drop zones (Home) still get the event first.
 * Returns an uninstaller.
 */
export function installDropGuard(doc: Document): () => void {
  const cancel = (e: Event) => {
    // Outside a drop zone, show the "no drop" cursor instead of a copy cursor.
    const dt = (e as Partial<DragEvent>).dataTransfer
    if (e.type === 'dragover' && !e.defaultPrevented && dt) dt.dropEffect = 'none'
    e.preventDefault()
  }
  doc.addEventListener('dragover', cancel)
  doc.addEventListener('drop', cancel)
  return () => {
    doc.removeEventListener('dragover', cancel)
    doc.removeEventListener('drop', cancel)
  }
}
