import { FolderPlus, RefreshCw, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DevicePicker } from '../components/DevicePicker'
import { SHORTCUTS } from '../lib/shortcuts'
import { S } from '../lib/strings'
import { useLibrary } from '../stores/library'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import v from './views.module.css'

export function SettingsView() {
  const folders = useLibrary((l) => l.folders)
  const mode = usePlayer((p) => p.state.mode)
  const expanded = useUi((u) => u.sidebarExpanded)
  const [removing, setRemoving] = useState<{ id: number; path: string } | null>(null)
  const lib = useLibrary.getState()
  return (
    <div className={v.view} style={{ maxWidth: 860 }}>
      <h1 className={v.title}>{S.settingsTitle}</h1>

      <section className={v.section}>
        <div className={v.toolbar}>
          <h2 className={v.sectionTitle}>{S.libraryFolders}</h2>
          <div className={v.actions}>
            <button className={v.ghost} onClick={() => void lib.rescan()}><RefreshCw size={16} /> {S.rescanAll}</button>
            <button className={v.primary} onClick={() => void lib.addFolder()}><FolderPlus size={16} /> {S.addFolder}</button>
          </div>
        </div>
        <ul aria-label={S.libraryFolders} style={{ listStyle: 'none', margin: 0, padding: 0, display: 'flex', flexDirection: 'column', gap: 8 }}>
          {folders.map((f) => (
            <li key={f.id} style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '10px 12px', background: 'var(--raised)', borderRadius: 6 }}>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="mono" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{f.path}</div>
                <div className={v.meta}>{S.trackCount(f.trackCount)} · {S.lastScan(f.lastScanAt)}{!f.available && <> · <span style={{ color: 'var(--danger)' }}>{S.unavailable}</span></>}</div>
              </div>
              <button className={v.ghost} aria-label={S.rescan} onClick={() => void lib.rescan(f.id)}><RefreshCw size={14} /></button>
              <button className={v.ghost} aria-label={S.remove} onClick={() => setRemoving({ id: f.id, path: f.path })}><Trash2 size={14} /></button>
            </li>
          ))}
        </ul>
      </section>

      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.audio}</h2>
        <div style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: '12px 16px', alignItems: 'center' }}>
          <span className="muted">{S.outputDevice}</span><DevicePicker />
          <span className="muted">{S.outputMode}</span>
          <div role="radiogroup" aria-label={S.outputMode} style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {(['processed', 'bitPerfect'] as const).map((m) => (
              <label key={m} style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <input type="radio" name="mode" checked={mode === m} onChange={() => void usePlayer.getState().setMode(m)} />
                {m === 'processed' ? S.processed : S.bitPerfect}
              </label>
            ))}
          </div>
          <span className="muted">{S.buffer}</span><span className="mono">{S.bufferInfo}</span>
        </div>
      </section>

      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.appearance}</h2>
        <label style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <input type="checkbox" checked={expanded} onChange={(e) => useUi.getState().setSidebar(e.target.checked)} /> {S.expandedSidebar}
        </label>
      </section>

      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.shortcuts}</h2>
        <dl style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: '6px 16px', margin: 0 }}>
          {SHORTCUTS.map((k) => <div key={k.keys} style={{ display: 'contents' }}><dt className="mono">{k.keys}</dt><dd style={{ margin: 0 }}>{k.action}</dd></div>)}
        </dl>
      </section>

      <section className={v.section}>
        <h2 className={v.sectionTitle}>{S.about}</h2>
        <p className="muted" style={{ margin: 0, userSelect: 'text' }}>{S.licences}</p>
      </section>

      {removing && (
        <ConfirmDialog title={S.removeFolderTitle} body={S.removeFolderBody(removing.path)} confirm={S.remove}
          onCancel={() => setRemoving(null)} onConfirm={() => { const id = removing.id; setRemoving(null); void lib.removeFolder(id) }} />
      )}
    </div>
  )
}
